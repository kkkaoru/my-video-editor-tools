//! Which steps of a job run at the same time and which in order, from what each step needs and what
//! the machine has. Jobs themselves run one after another (`service::work_through_queue`), so the
//! budget belongs to one job at a time.

use crate::catalog::{self, Hardware, RunsOn, SpeakerPlan};
use crate::eta::Stage;
use crate::pipeline::wants_faces;
use crate::settings::Settings;
use serde::Serialize;

/// Left to the system.
/// shortcut: an estimate, not a measurement; replace it when a low-memory machine has been measured.
const RESERVED_RAM_GIB: u64 = 4;
/// The app itself holds 0.55 to 1.03 GB through a whole job, beside whichever step runs.
const APP_RAM_GIB: u64 = 1;

/// What one step holds while it runs. The values come from the catalog, the one place they are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepNeed {
    pub stage: Stage,
    pub on: RunsOn,
    pub ram_gib: u64,
    pub gpu_memory_gib: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    pub ram_gib: u64,
    pub gpu_memory_gib: u64,
    /// One memory holds both, as on a Mac: the two are added, never counted twice.
    pub unified: bool,
}

impl Budget {
    #[must_use]
    pub fn of(hardware: &Hardware) -> Self {
        Self {
            ram_gib: hardware
                .memory_gib
                .saturating_sub(RESERVED_RAM_GIB + APP_RAM_GIB),
            gpu_memory_gib: hardware.gpu_memory_gib().unwrap_or(0),
            unified: hardware.is_mac,
        }
    }

    fn holds(self, ram_gib: u64, gpu_memory_gib: u64) -> bool {
        if self.unified {
            ram_gib + gpu_memory_gib <= self.ram_gib
        } else {
            ram_gib <= self.ram_gib && gpu_memory_gib <= self.gpu_memory_gib
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExecutionPlan {
    /// In order; the steps of a group run at the same time.
    pub groups: Vec<Vec<Stage>>,
    /// The most RAM the steps hold at once, without the app (`execution_plan` adds it).
    pub peak_ram_gib: u64,
    pub peak_gpu_memory_gib: u64,
}

/// The steps whose results a step reads. The words decide where voices are separated.
fn reads(stage: Stage) -> &'static [Stage] {
    match stage {
        Stage::Separate => &[Stage::Transcribe, Stage::Diarize],
        Stage::Retranscribe => &[Stage::Separate],
        _ => &[],
    }
}

fn total(group: &[StepNeed], read: impl Fn(&StepNeed) -> u64) -> u64 {
    group.iter().map(read).sum()
}

/// Takes the steps in order, and starts beside each the later ones that can run with it: their inputs
/// are ready, nothing else is on their device (two on one device only take turns), and the memory holds
/// them all. A step that fits alone was already checked against the machine by the catalog.
#[must_use]
pub fn schedule(needs: &[StepNeed], budget: Budget) -> ExecutionPlan {
    let mut waiting = needs.to_vec();
    let mut finished = Vec::new();
    let mut plan = ExecutionPlan {
        groups: Vec::new(),
        peak_ram_gib: 0,
        peak_gpu_memory_gib: 0,
    };
    while !waiting.is_empty() {
        let mut group: Vec<StepNeed> = Vec::new();
        waiting.retain(|need| {
            let ready = reads(need.stage).iter().all(|input| {
                finished.contains(input) || !needs.iter().any(|other| other.stage == *input)
            });
            let free = group.iter().all(|running| running.on != need.on);
            let holds = budget.holds(
                total(&group, |step| step.ram_gib) + need.ram_gib,
                total(&group, |step| step.gpu_memory_gib) + need.gpu_memory_gib,
            );
            let starts = ready && free && (group.is_empty() || holds);
            if starts {
                group.push(*need);
            }
            !starts
        });
        finished.extend(group.iter().map(|step| step.stage));
        plan.peak_ram_gib = plan.peak_ram_gib.max(total(&group, |step| step.ram_gib));
        plan.peak_gpu_memory_gib = plan
            .peak_gpu_memory_gib
            .max(total(&group, |step| step.gpu_memory_gib));
        plan.groups
            .push(group.iter().map(|step| step.stage).collect());
    }
    plan
}

fn need_of(stage: Stage, id: &str, on: RunsOn) -> Option<StepNeed> {
    let entry = catalog::find_entry(id)?;
    let (ram_gib, gpu_memory_gib) = match on {
        RunsOn::Gpu => (entry.min_ram_with_gpu_gib, entry.min_gpu_memory_gib),
        RunsOn::Cpu => (entry.min_ram_gib, 0),
    };
    Some(StepNeed {
        stage,
        on,
        ram_gib,
        gpu_memory_gib,
    })
}

/// The steps a job with these settings runs, in the order they are considered.
fn job_needs(
    settings: &Settings,
    hardware: &Hardware,
    speaker_plan: Option<&SpeakerPlan>,
) -> Vec<StepNeed> {
    let whisper_on = catalog::whisper_runs_on(hardware);
    [
        need_of(Stage::Transcribe, catalog::WHISPER_ID, whisper_on),
        speaker_plan.and_then(|plan| need_of(Stage::Diarize, plan.diarize, plan.diarize_on)),
        wants_faces(settings)
            .then(|| need_of(Stage::Faces, &settings.face_model, RunsOn::Cpu))
            .flatten(),
        speaker_plan.and_then(|plan| need_of(Stage::Separate, plan.separator, plan.separate_on)),
        speaker_plan.and_then(|_| need_of(Stage::Retranscribe, catalog::WHISPER_ID, whisper_on)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[must_use]
pub fn execution_plan(
    settings: &Settings,
    hardware: &Hardware,
    speaker_plan: Option<&SpeakerPlan>,
) -> ExecutionPlan {
    let plan = schedule(
        &job_needs(settings, hardware, speaker_plan),
        Budget::of(hardware),
    );
    ExecutionPlan {
        peak_ram_gib: plan.peak_ram_gib + APP_RAM_GIB,
        ..plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Gpu, GpuVendor};

    fn step(stage: Stage, on: RunsOn, ram_gib: u64, gpu_memory_gib: u64) -> StepNeed {
        StepNeed {
            stage,
            on,
            ram_gib,
            gpu_memory_gib,
        }
    }

    fn apart(ram_gib: u64, gpu_memory_gib: u64) -> Budget {
        Budget {
            ram_gib,
            gpu_memory_gib,
            unified: false,
        }
    }

    #[test]
    fn a_gpu_step_and_a_cpu_step_run_together_when_both_fit() {
        let plan = schedule(
            &[
                step(Stage::Transcribe, RunsOn::Gpu, 2, 2),
                step(Stage::Diarize, RunsOn::Cpu, 2, 0),
            ],
            apart(4, 2),
        );
        assert_eq!(
            plan,
            ExecutionPlan {
                groups: vec![vec![Stage::Transcribe, Stage::Diarize]],
                peak_ram_gib: 4,
                peak_gpu_memory_gib: 2,
            }
        );
    }

    #[test]
    fn steps_go_in_order_when_the_ram_does_not_hold_both() {
        let plan = schedule(
            &[
                step(Stage::Transcribe, RunsOn::Gpu, 2, 2),
                step(Stage::Diarize, RunsOn::Cpu, 2, 0),
            ],
            apart(3, 2),
        );
        assert_eq!(
            plan,
            ExecutionPlan {
                groups: vec![vec![Stage::Transcribe], vec![Stage::Diarize]],
                peak_ram_gib: 2,
                peak_gpu_memory_gib: 2,
            }
        );
    }

    #[test]
    fn two_steps_on_one_device_never_run_together() {
        let plan = schedule(
            &[
                step(Stage::Transcribe, RunsOn::Gpu, 2, 2),
                step(Stage::Diarize, RunsOn::Gpu, 1, 2),
                step(Stage::Faces, RunsOn::Cpu, 1, 0),
            ],
            apart(64, 64),
        );
        assert_eq!(
            plan.groups,
            vec![vec![Stage::Transcribe, Stage::Faces], vec![Stage::Diarize]]
        );
    }

    #[test]
    fn everything_on_the_cpu_goes_in_order_and_needs_the_largest_step() {
        let plan = schedule(
            &[
                step(Stage::Transcribe, RunsOn::Cpu, 3, 0),
                step(Stage::Diarize, RunsOn::Cpu, 2, 0),
            ],
            apart(64, 0),
        );
        assert_eq!(
            plan,
            ExecutionPlan {
                groups: vec![vec![Stage::Transcribe], vec![Stage::Diarize]],
                peak_ram_gib: 3,
                peak_gpu_memory_gib: 0,
            }
        );
    }

    #[test]
    fn a_single_memory_is_not_counted_twice() {
        let needs = [
            step(Stage::Transcribe, RunsOn::Gpu, 2, 2),
            step(Stage::Faces, RunsOn::Cpu, 2, 0),
        ];
        let unified = |ram_gib| Budget {
            ram_gib,
            gpu_memory_gib: 8,
            unified: true,
        };
        assert_eq!(
            schedule(&needs, unified(5)).groups,
            vec![vec![Stage::Transcribe], vec![Stage::Faces]]
        );
        assert_eq!(
            schedule(&needs, unified(6)).groups,
            vec![vec![Stage::Transcribe, Stage::Faces]]
        );
    }

    #[test]
    fn separation_waits_for_the_words_and_the_speakers_however_much_memory_there_is() {
        let plan = schedule(
            &[
                step(Stage::Transcribe, RunsOn::Gpu, 2, 2),
                step(Stage::Diarize, RunsOn::Gpu, 1, 2),
                step(Stage::Separate, RunsOn::Cpu, 4, 0),
                step(Stage::Retranscribe, RunsOn::Gpu, 2, 2),
            ],
            apart(64, 64),
        );
        assert_eq!(
            plan.groups,
            vec![
                vec![Stage::Transcribe],
                vec![Stage::Diarize],
                vec![Stage::Separate],
                vec![Stage::Retranscribe],
            ]
        );
    }

    #[test]
    fn a_step_too_large_for_the_budget_still_runs_alone() {
        let plan = schedule(&[step(Stage::Transcribe, RunsOn::Cpu, 3, 0)], apart(0, 0));
        assert_eq!(plan.groups, vec![vec![Stage::Transcribe]]);
        assert_eq!(plan.peak_ram_gib, 3);
    }

    #[test]
    fn no_steps_make_an_empty_plan() {
        assert_eq!(
            schedule(&[], apart(8, 8)),
            ExecutionPlan {
                groups: Vec::new(),
                peak_ram_gib: 0,
                peak_gpu_memory_gib: 0,
            }
        );
    }

    fn windows(memory_gib: u64, gpu_memory_gib: Option<u64>) -> Hardware {
        Hardware {
            is_mac: false,
            memory_gib,
            gpu: gpu_memory_gib.map(|memory_gib| Gpu {
                vendor: GpuVendor::Nvidia,
                name: "NVIDIA GeForce RTX 3060".to_owned(),
                memory_gib,
            }),
        }
    }

    fn fixed_position() -> Settings {
        let mut settings = Settings::default();
        settings.placement.anchor = crate::settings::Anchor::VideoBottom;
        settings
    }

    fn speakers(on: RunsOn, separate_on: RunsOn) -> SpeakerPlan {
        SpeakerPlan {
            diarize: catalog::NEMOTRON35_SPEAKER_ID,
            whisper_on: RunsOn::Gpu,
            diarize_on: on,
            separate_on,
            separator: catalog::MOSSFORMER2_ID,
            deep: false,
        }
    }

    #[test]
    fn a_budget_keeps_ram_for_the_system_and_a_mac_has_one_memory() {
        assert_eq!(Budget::of(&windows(16, Some(8))), apart(11, 8));
        assert_eq!(Budget::of(&windows(2, None)), apart(0, 0));
    }

    #[test]
    fn one_person_runs_only_whisper() {
        let plan = execution_plan(&fixed_position(), &windows(16, Some(8)), None);
        assert_eq!(
            plan,
            ExecutionPlan {
                groups: vec![vec![Stage::Transcribe]],
                peak_ram_gib: 3,
                peak_gpu_memory_gib: 2,
            }
        );
    }

    #[test]
    fn the_catalog_supplies_each_steps_needs() {
        let plan = execution_plan(
            &fixed_position(),
            &windows(16, Some(8)),
            Some(&speakers(RunsOn::Cpu, RunsOn::Cpu)),
        );
        assert_eq!(
            plan,
            ExecutionPlan {
                groups: vec![
                    vec![Stage::Transcribe, Stage::Diarize],
                    vec![Stage::Separate],
                    vec![Stage::Retranscribe],
                ],
                peak_ram_gib: 6,
                peak_gpu_memory_gib: 2,
            }
        );
    }

    #[test]
    fn faces_run_beside_whisper_on_the_gpu() {
        let mut settings = Settings::default();
        settings.placement.anchor = crate::settings::Anchor::FaceBelow;
        let plan = execution_plan(&settings, &windows(16, Some(8)), None);
        assert_eq!(plan.groups, vec![vec![Stage::Transcribe, Stage::Faces]]);
    }

    #[test]
    fn a_model_the_catalog_does_not_know_needs_no_step() {
        assert_eq!(need_of(Stage::Diarize, "nope", RunsOn::Cpu), None);
    }
}
