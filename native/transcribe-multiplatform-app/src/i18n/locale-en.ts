// Runs with bun: English UI text.
import type { Messages } from "./messages.ts";

export const enMessages: Messages = {
  appName: "Transcribe Captions",
  tabJobs: "Transcribe",
  tabStyle: "Caption look and position",
  tabSettings: "Settings",
  uiLanguageLabel: "Display language",
  uiLanguageSystem: "Same as the system",
  uiThemeLabel: "Appearance",
  uiThemeSystem: "Same as the system",
  uiThemeLight: "Light",
  licensesOpen: "Open source licenses",
  licensesLoading: "Loading the licenses",
  licensesFailed: "The license list could not be read.",
  licensesNoText: "This package does not include a license text.",
  licensesUnknown: "Not declared",
  licensesClose: "Close",
  uiThemeDark: "Dark",
  updateCheck: "Check for updates",
  updateChecking: "Checking for updates",
  updateUpToDate: "This is the latest version.",
  updateCheckFailed: "Could not check for updates: {detail}",
  updateAvailable: "Version {version} is available",
  updateInstall: "Update and restart",
  updateRetry: "Try the update again",
  updateBusy: "You can update when the current videos and model downloads are finished.",
  updateInstalling: "Downloading the update. The app restarts when it is ready.",
  updateProgressLabel: "Update download",
  updateInstallFailed: "The update did not finish: {detail}",
  tabHelp: "How to use",
  tabModels: "Models",
  noticeSpeakersOnCpu:
    "Telling speakers apart runs on the CPU until you download the GeForce runtime in Models, so it is slower.",
  noticeCpuOnly: "No GPU this version can use was found, so everything runs on the CPU. It is slower.",
  dropTitle: "Drop videos or subtitle files here",
  dropHint: "Videos become subtitle files for video editing software, written next to each video.",
  dropHover: "Release to add the files",
  pickVideos: "Choose videos",
  pickSubtitles: "Choose subtitle files",
  subtitleFilterName: "Subtitle files",
  dropProfileHint: "Dropped subtitle files are written again as new files with the saved profile {name}.",
  dropCurrentHint: "Dropped subtitle files are written again as new files with the current caption settings.",
  speakerCountHelpLabel: "Why set the number of speakers?",
  speakerCountHelp:
    "Speaker colours depend on telling voices apart. Saying how many people speak keeps similar voices from merging and one voice from splitting. With 1 speaker, speaker separation is skipped, so it is faster and needs no speaker model.",
  speakerLoadLight: "Light",
  speakerLoadHeavy: "Heavy",
  speakerOptionLoad: "{label} ({load})",
  speakerLoadLightDetail: "Uses Whisper only, with no voice separation or speaker separation.",
  speakerLoadHeavyDetail:
    "Tells the voices apart, so it takes much longer and uses much more RAM or GPU memory.",
  speakerFiguresNote:
    "Whisper on the CPU took about {whisperCpuMinutes} min on one video of about an hour, with no passages redone. These figures were measured on one Mac, and vary by video and computer. RAM and GPU memory on Windows have not been measured.",
  memoryUnified: "unified memory (shared RAM and GPU memory)",
  memoryRam: "RAM",
  memoryGpu: "GPU memory",
  speakerPlanRuns: "Runs on: Whisper {whisper}, speakers {diarize}.",
  speakerPlanRunsSeparate: " Voice separation {separate}.",
  speakerPlanDeep:
    "Three or more people at once are separated twice, so it takes longer. Up to four rows appear.",
  separationBackend: "How voice separation runs",
  separationBackends: { onnx: "CPU (ONNX Runtime)", coreml: "Mac GPU (Core ML)" },
  speakerPlanLine: "Runs: {models}.",
  speakerFactLabels: {
    runs: "What runs",
    rows: "Rows",
    memory: "RAM and GPU memory",
    time: "Time (Whisper not included)",
  },
  speakerFacts: {
    runs: "Nemotron 3.5 encoder and MossFormer2",
    rows: "Voices at once appear on several rows.",
    memory:
      "On a Mac, {memoryUnified}: {encoderGb} GB for the Nemotron 3.5 encoder, about {separateCoreMlGb} GB (estimate) for MossFormer2 on Core ML, {separateGb} GB on the CPU with ONNX Runtime ({separateSeconds}-second passages). About {jobPeakGb} GB at the job's peak.",
    time: "Nemotron 3.5 encoder: per hour of audio, about {encoderGpuSeconds} seconds on a Mac's GPU, about {encoderCpuSeconds} seconds on the CPU. Voice separation, measured on one video of about an hour: about {separateCoreMlMinutes} min on Core ML, about {separateCpuMinutes} min on the CPU. It changes with how much of the video it handles. Three or more people at once are separated twice, which takes longer.",
  },
  openModelsTab: "Open the “{tabModels}” tab",
  runWithOneSpeaker: "Run with 1 speaker",
  roleSeparate: "voice separation",
  badgeUsedBySteps: "Used for 2 or more speakers",
  speakerRolesHint: "Needed when 2 or more people speak, or when the number is detected automatically.",
  profilesHeading: "Profiles",
  profilesHint:
    "Save the caption look and position under a name and restore it later. Models and the spoken language are not part of a profile.",
  profileName: "Profile name",
  profileSave: "Save as a new profile",
  profileEmpty: "No profiles saved yet.",
  profileSelected: "Selected",
  profileChanged: "Unsaved changes",
  profileLoad: "Load {name}",
  profileOverwrite: "Overwrite {name}",
  profileOverwriteTitle: "Overwrite {name}?",
  profileOverwriteDescription:
    "The profile's saved look is replaced with the current settings. This cannot be undone.",
  profileDelete: "Delete {name}",
  profileDeleteTitle: "Delete {name}?",
  profileDeleteDescription:
    "The profile is removed. The current settings and existing subtitle files stay as they are. This cannot be undone.",
  profileExport: "Export {name}",
  profileImport: "Import from a file",
  profileFileFilter: "Profiles",
  resetLook: "Restore the original look and position",
  resetLookTitle: "Restore the original look and position?",
  resetLookCurrent:
    "The current look and position go back to the app's original settings. Other profiles, the number of speakers, models and the language do not change. This cannot be undone.",
  resetLookProfile:
    "The look and position of the profile {name} go back to the app's original settings. Other profiles, the number of speakers, models and the language do not change. This cannot be undone.",
  resetLookConfirm: "Restore",
  dialogCancel: "Cancel",
  fontFamily: "Font family",
  fontSearch: "Search the fonts",
  fontHint: "Used for every speaker. Type to narrow down the list.",
  fontLoading: "Reading the installed fonts",
  fontNoMatch: "No installed font matches",
  fontNotInstalled:
    "{font} is not installed on this computer. The preview uses a substitute, and the subtitle files still name it.",
  downloadAllLabel: "Download what is missing",
  downloadAll: "{label}: {count} ({size})",
  resetModels: "Reset models and download again",
  resetModelsTitle: "Reset the models and download them again?",
  resetModelsDescription:
    "Everything this app downloaded in {path} is deleted, then the models this computer needs are downloaded again. Folders you added are never touched. This cannot be undone.",
  resetModelsConfirm: "Delete and download again",
  videoFilterName: "Videos",
  readinessMissing:
    "No model is available for: {roles}. Download one in Models, or add a folder that already has it.",
  roleTranscribe: "transcription",
  roleDiarize: "speaker separation",
  roleFace: "face detection",
  roleRuntime: "runtime",
  jobQueued: "Waiting in line",
  jobCancelled: "Cancelled",
  jobCancel: "Cancel {name}",
  jobRelayout: "Write {name} again with the current position and colours",
  jobDone: "Finished: {cues} captions, {speakers} speakers",
  jobDoneOneSpeaker: "Finished: {cues} captions, 1 speaker",
  jobFaceless: "Captions where no face was found were placed at the bottom of the video.",
  jobSpeakersMissing: "The speaker model heard nobody in this video, so every caption has one colour.",
  jobFontMissing: "The chosen font is not on this computer, so the caption widths are an estimate.",
  jobNoPercent: "this model reports no percentage",
  speakerCount: "Number of speakers",
  subtitleFormat: "Subtitle file format",
  speakerCountAuto: "Detect automatically",
  speakerCountValue: "{count} speakers",
  speakerCountOne: "1 speaker",
  jobFewerSpeakers:
    "{requested} speakers were requested but only {found} were found. Short recordings or similar voices can do this.",
  jobStep: "Step {number} of {total}: {stage}",
  jobEstimating: "Estimating the time left",
  jobRemaining: "About {minutes} min left",
  jobRemainingUnderMinute: "Less than a minute left",
  stepStates: { done: "done", current: "in progress", pending: "waiting" },
  outputPurposes: {
    drt: "DaVinci Resolve timeline of Text+ clips: colours, outlines, outer edges and shadows",
    ttml: "Styled subtitles (colour, outline, position)",
    srt: "Subtitles with words and times (colours may not show; no outlines or positions)",
    fcpxml: "Timeline with titles",
    json: "Transcript data for writing again",
  },
  jobProgressLabel: "Progress of {name}",
  jobOutputs: "Files written",
  stages: {
    convert_model: "Converting the model",
    extract_audio: "Extracting audio",
    transcribe: "Transcribing",
    diarize: "Separating speakers",
    separate: "Separating overlapping voices",
    retranscribe: "Transcribing the separated voices",
    faces: "Detecting faces",
    export: "Writing subtitle files",
  },
  positionHeading: "Caption position",
  positionLabel: "Place captions",
  anchorFaceAbove: "Above the face",
  anchorFaceBelow: "Below the face",
  anchorVideoTop: "Top of the video",
  anchorVideoBottom: "Bottom of the video",
  offsetX: "Fine-tune left and right (percent of width)",
  offsetY: "Fine-tune up and down (percent of height)",
  safeZoneHint:
    "The strip at each side of the video where subtitles are not placed. The preview shows it as a dark band.",
  safeLeft: "Left safe zone (percent of width)",
  safeRight: "Right safe zone (percent of width)",
  faceModel: "Face detection model",
  speechLanguage: "Spoken language",
  languageAuto: "Detect automatically",
  sizeHeading: "Caption size",
  sizeHint: "Sizes follow each video's resolution automatically. These are proportions of the video.",
  fontSize: "Text size (percent of the short edge)",
  captionWidth: "Longest line (percent of the width)",
  captionLineHeight: "Line spacing (times the text size)",
  contrastWarning:
    "{name}: the fill and outline contrast is {ratio}:1, below the WCAG AA minimum of 4.5:1. Choose colours further apart.",
  previewHeading: "Preview",
  previewFrameLabel: "Preview size",
  previewDescription: "Preview caption at {x}% across and {y}% down the {size} frame.",
  previewFontPixels: "Text size at this resolution: {px} px.",
  previewSample: "Outlined captions change colour with each speaker",
  previewLandscape: "1920×1080",
  previewVertical: "Vertical 1080×1920",
  previewUhd: "4K",
  speakersHeading: "Speaker colours (up to {count} speakers)",
  speakersHint:
    "Choose a speaker to change their colours and thickness. The preview shows that speaker's captions. Thickness and position are a percent of the text size.",
  speakerName: "Speaker {number}",
  colorColumn: "Colour",
  fillWidth: "Fill thickness (percent of the text size)",
  outlineWidth: "Outline thickness (percent of the text size)",
  rimWidth: "Outer edge thickness (percent of the text size)",
  layerWidth: "Thickness (percent of the text size)",
  colorTransparent: "Transparent",
  colorCustom: "Any colour",
  colorNames: {
    white: "White",
    black: "Black",
    red: "Red",
    orange: "Orange",
    yellow: "Yellow",
    green: "Green",
    blue: "Blue",
    purple: "Purple",
  },
  fillLabel: "Fill",
  outlineLabel: "Outline",
  rimLabel: "Outer edge",
  formatLimits: {
    fcpxml: "FCPXML shows no shadow. Choose DRT to use one.",
    ttml: "TTML shows no outer edge or shadow. Choose DRT to use them.",
    srt: "SRT keeps only the text colour: no outlines, outer edges or shadows. Choose DRT to use them.",
  },
  shadowLabel: "Shadow",
  shadowColor: "Shadow colour",
  shadowX: "Shadow left/right (percent of the text size)",
  shadowXShort: "Shadow left/right",
  shadowY: "Shadow up/down (percent of the text size; minus is down)",
  shadowYShort: "Shadow up/down",
  shadowBlur: "Shadow blur (0 to 50)",
  shadowBlurShort: "Shadow blur",
  addSpeaker: "Add a speaker",
  removeSpeaker: "Remove a speaker",
  resetPalette: "Restore the first colours",
  modelsHeading: "Models",
  noRecommendedSpeakerModel:
    "No model for telling speakers apart is recommended for this computer. You can still make subtitles for 1 speaker.",
  modelsHint:
    "The models suited to this computer are marked as recommended. Models you already downloaded are found and reused.",
  addModelFolder: "Add a folder with existing models",
  modelFolders: "Folders searched",
  badgeRecommended: "Recommended here",
  badgePresent: "Available",
  download: "Download {name} ({size})",
  downloadCancel: "Cancel the download of {name}",
  downloadProgress: "{done} of {total}",
  downloadProgressLabel: "Download of {name}",
  deleteModel: "Delete the downloaded copy of {name}",
  sourceLabel: "Source: {url}",
  locationLabel: "Location: {path}",
  loadFailed: "Something went wrong: {detail}",
  errors: {
    no_video_stream: "This file has no video. Choose a video file.",
    no_audio_stream: "This video has no sound, so there is nothing to transcribe.",
    unreadable_size: "The size of this video could not be read. The file may be damaged.",
    unreadable_duration: "The length of this video could not be read. The file may be damaged.",
    no_words: "No words were found in the audio.",
    model_missing: "{model} is not installed. Download it in Models.",
    no_model_for_role: "No model is installed for {role}. Download one in Models.",
    needs_memory:
      "{model} needs {required} GB of {memory}; this computer has {available} GB. Choose a smaller model.",
    transcript_missing:
      "{file} has no transcript beside it, so it cannot be written again. Keep the .transcript.json file with the same name in the same folder.",
    profile_exists:
      "A profile named {name} already exists. Choose another name, or overwrite it in the list.",
    profile_missing: "The profile {name} no longer exists.",
    models_busy:
      "Models cannot be reset while a job or a download is running. Wait for it to finish or cancel it, then try again.",
    models_missing:
      "The videos were not added because no model is available for: {roles}. Download them in Models, or add a folder that already has them.",
    speaker_models_missing:
      "These models are needed for 2 or more speakers, or for automatic detection: {roles}. Download them in Models, or set the number of speakers to 1 and add the video again.",
    update_busy:
      "An update cannot start while videos or model downloads are running. Wait until they are finished, then try again.",
    no_supported_files:
      "This file cannot be added. Choose a video (.mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts) or a subtitle file (.drt .fcpxml .srt .ttml .transcript.json).",
    profile_invalid: "{file} cannot be read as a profile. Choose a file this app exported.",
    internal: "{detail}",
    cancelled: "Cancelled",
  },
  modelNotes: {
    "whisper-large-v3-turbo": "Transcription. The same model runs on Mac (Metal) and Windows (Vulkan).",
    "nemotron-3.5-speaker":
      "An encoder used only to tell speakers apart. It runs on the GPU or the CPU. Whisper turbo does the transcribing. A full Nemotron 3.5 model already in your folders also works.",
    "mossformer2-ss-16k-coreml":
      "Splits overlapping voices on the Mac's GPU with Core ML. Used by “High quality”.",
    "mossformer2-ss-16k": "Splits overlapping voices. Used by “High quality”.",
    yunet: "Real faces. Included with the app; gives the same result on Mac and Windows.",
    "avatar-face-yolo": "Avatar faces. Works once the exported ONNX file is in a model folder.",
    "geforce-runtime":
      "Optional. Makes telling speakers apart faster on the GPU. Without it, this runs on the CPU, and it returns to the CPU if the GPU fails.",
  },
  help: {
    version: "Version {version}",
    intro:
      "This app turns the words spoken in a video into subtitles, with a different colour for each person. You can use the subtitles in video editing software.",
    sections: {
      start: {
        heading: "First steps",
        parts: [
          {
            heading: "Install the app",
            intro: "Download the file that fits your computer, then install it.",
            steps: [
              "Choose the file for your computer from the list below.",
              "On a Mac, open the zip and move the app to the Applications folder.",
              "On Windows, open the exe and follow the steps.",
            ],
            notes: [
              "Mac: Transcribe-Captions-macos-arm64.zip",
              "Windows with a GeForce: Transcribe-Captions-windows-geforce-x64-setup.exe",
              "Windows with a Radeon: Transcribe-Captions-windows-radeon-x64-setup.exe",
              "Windows with neither: either exe works. It runs on the CPU only, so it takes longer.",
              "If a Mac stops you: open System Settings > Privacy & Security and press “Open Anyway”.",
              "If Windows stops you: press “More info”, then “Run anyway”.",
            ],
          },
          {
            heading: "Download the models",
            intro:
              "Models are the data the app uses to turn speech into text. You only need to get them once.",
            steps: [
              "Open the “{tabModels}” tab.",
              "Press “{downloadAllLabel}”.",
              "Wait until each model's bar reaches the end.",
            ],
            notes: [
              "A finished model shows “{badgePresent}”.",
              "If nothing is missing, this button does not appear.",
              "You need the internet. The files are large, so this takes time.",
              "If a download stops, press the button again and it goes on from where it stopped.",
            ],
          },
          {
            heading: "Use models you already have",
            intro: "If you downloaded models before, you do not need to download them again.",
            steps: [
              "Open the “{tabModels}” tab.",
              "Press “{addModelFolder}”.",
              "Choose the folder that holds the models.",
            ],
            notes: ["Models it finds show “{badgePresent}”."],
          },
        ],
      },
      transcribe: {
        heading: "Make subtitles from a video",
        parts: [
          {
            heading: "Add a video",
            intro: "When you add a video, the app starts making subtitles.",
            steps: [
              "Open the “{tabJobs}” tab.",
              "“{speakerCount}” starts at 1. If more people speak, choose how many. If you are not sure, choose “{speakerCountAuto}”.",
              "Choose the file type in “{subtitleFormat}”. It starts at DRT. The line under it says what each type is for.",
              "Drag the video into the box and let go. To drag means to hold it down while you move it. You can also press “{pickVideos}”.",
            ],
            notes: [
              "Press the “?” next to the number to read why it helps.",
              "If only one person speaks, leave it at 1. It finishes faster.",
              "1 speaker is light: only Whisper runs. Automatic detection and 2 or more speakers are heavy: Nemotron and MossFormer2 also run, so it takes much longer and uses much more RAM or GPU memory.",
              "You can add these videos: .mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts. Subtitle files can be .drt .fcpxml .srt .ttml .transcript.json. With any other file, the app says it cannot be added.",
              "If you add several videos, they are done one at a time.",
              "The first time, the app also changes a model into a form it can use.",
              "If the subtitles are in the wrong language, choose it in “{speechLanguage}” on the “{tabStyle}” tab.",
            ],
          },
          {
            heading: "Watch the progress",
            intro: "After you add a video, a list appears under the box.",
            steps: [],
            notes: [
              "It shows the current step and how many steps there are.",
              "The bar shows the progress of the whole job.",
              "It also shows about how much time is left.",
              "To stop, press the button next to the video's name.",
            ],
          },
          {
            heading: "The files you get",
            intro:
              "Two files appear in the same folder as the video: the subtitle file in the type you chose, and a .transcript.json file. Older files are kept. If the name is taken, the new files get -2 added to their name.",
            steps: [],
            notes: [
              ".drt: Text+ subtitles for DaVinci Resolve, with colours, outlines, outer edges and shadows.",
              ".fcpxml: subtitles video editing software can read, together with the video.",
              ".ttml: subtitles with colours and positions.",
              ".srt: subtitles with words and times. Colours may not show. There are no outlines or positions.",
              ".transcript.json: the data used to write subtitles again. Please keep it.",
            ],
          },
          {
            heading: "When 2 or more people speak",
            intro:
              "With automatic detection or 2 or more speakers, Whisper turbo, the Nemotron 3.5 encoder and MossFormer2 run. Whisper turbo always does the transcribing; Nemotron only tells speakers apart.",
            steps: [],
            notes: [
              "Voices at once appear on several rows. On a Mac, {memoryUnified}: {encoderGb} GB for the Nemotron 3.5 encoder, about {separateCoreMlGb} GB (estimate) for MossFormer2 on Core ML. Three or more people at once are separated twice, which takes longer.",
              "On a Mac, “{separationBackend}” chooses the CPU or the Mac's GPU for voice separation.",
              "Nemotron 3.5 encoder: per hour of audio, about {encoderGpuSeconds} seconds on a Mac's GPU, about {encoderCpuSeconds} seconds on the CPU. Voice separation, measured on one video of about an hour: about {separateCoreMlMinutes} min on Core ML, about {separateCpuMinutes} min on the CPU. It changes with how much of the video it handles.",
              "{speakerFiguresNote}",
            ],
          },
        ],
      },
      editor: {
        heading: "Open the subtitles in your video editor",
        parts: [
          {
            heading: "Bring the subtitles into DaVinci Resolve",
            intro:
              "A .drt file brings the subtitles in as Text+ clips, with their colours, outlines, outer edges and shadows.",
            steps: [
              "Open a project in DaVinci Resolve.",
              "From the menu, choose File > Import > Timeline...",
              "Choose the .drt file the app made.",
              "On the screen that asks, press Import.",
              "The subtitles come in as Text+ clips. Select a clip to change the words under Text in the Inspector, and the colours and edges under Shading.",
            ],
            notes: [
              "The numbers of Select Element: 1 is the fill, 2 the outline, 4 the outer edge, 3 the shadow, 5 the fill thickness.",
              "For a new project, “Automatically set project setting” can stay on. When you add to a project you are editing, turn it off so the resolution does not change.",
              "Do not move the video file.",
            ],
          },
          {
            heading: "Bring in an FCPXML file",
            intro:
              "An FCPXML file brings in the video and the subtitles at once, with their colours, outlines and positions.",
            steps: [
              "Open a project in your video editor. A project holds all your editing work.",
              "Bring in the timeline from the menu (for example, File > Import > Timeline).",
              "Choose the .fcpxml file the app made.",
              "The video and subtitles appear on a timeline. A timeline is where a video is laid out in time order.",
            ],
            notes: [
              "Each subtitle is 2 or 3 layers on top of each other. To change the words, change every layer.",
            ],
          },
          {
            heading: "When to use the other types",
            intro: "To make another type, choose it in “{subtitleFormat}” before you add the video.",
            steps: [],
            notes: [
              "To bring in a .ttml file, choose Import Subtitle in the Media Pool. The Media Pool is where your videos and subtitles are kept.",
              "Use the .srt file to fix words or times inside your video editor. If this app writes the subtitles again, your fixes are not included.",
              "Do not use the FCPXML and TTML files on one timeline. The subtitles would show twice.",
            ],
          },
        ],
      },
      look: {
        heading: "Change how subtitles look and where they sit",
        parts: [
          {
            heading: "Change the look and position",
            intro:
              "Changes show in the preview right away and are saved by themselves. The preview shows how the subtitles will look.",
            steps: [
              "Open the “{tabStyle}” tab.",
              "Under “{positionHeading}”, choose “{anchorFaceAbove}”, “{anchorFaceBelow}”, “{anchorVideoTop}” or “{anchorVideoBottom}”.",
              "Use the slider to set the text size. A slider is a handle you move left and right. You can type a number too.",
              "Choose a speaker, then set the fill, outline, outer edge and shadow. Press a colour square to choose a colour. “Transparent” removes the outer edge or the shadow.",
            ],
            notes: [
              "The preview is on the right on a wide screen and at the top on a narrow one. It stays in view as you scroll.",
              "To keep subtitles off the left and right edges, set the safe zones under “{positionHeading}”. The dark bands in the preview show them.",
            ],
          },
          {
            heading: "Choose a font",
            intro:
              "A font is the shape of the letters. You choose from the fonts on this computer, and everyone's subtitles use the same one.",
            steps: [
              "Under “{fontFamily}”, press the place that shows the font's name.",
              "Type part of the font's name to shorten the list.",
              "Press the font you want.",
            ],
            notes: [],
          },
          {
            heading: "Save and load a profile",
            intro: "A profile is a set of look and position settings saved under a name.",
            steps: [
              "Type a name.",
              "Press “{profileSave}”.",
              "To use it later, press its load button in the list.",
            ],
            notes: [
              "The app asks you first before it overwrites or deletes a profile.",
              "Models and the spoken language are not saved in a profile.",
            ],
          },
          {
            heading: "Use a profile on another computer",
            intro: "Export a profile to a file, and you can use it on another computer.",
            steps: [
              "Press a profile's export button in the list and choose where to save.",
              "Copy the file to the other computer.",
              "On the other computer, press “{profileImport}” and choose the file.",
            ],
            notes: [
              "An imported profile appears in the list.",
              "If the name is taken, it comes in under a different name instead of replacing anything.",
            ],
          },
          {
            heading: "Go back to the original look",
            intro: "You can put the look and position back to the app's original settings.",
            steps: ["Open the “{tabStyle}” tab.", "Press “{resetLook}”.", "Read the message, then go on."],
            notes: [
              "If you are using a profile, only that profile goes back.",
              "The number of speakers, models, the language and other profiles do not change.",
            ],
          },
        ],
      },
      rewrite: {
        heading: "Give finished subtitles a new look",
        parts: [
          {
            heading: "Add a subtitle file",
            intro:
              "You can change the look of subtitles without making them again from the sound. It is usually quick. It takes longer if the app has to look for faces.",
            steps: [
              "Load the profile you want. If none is loaded, the current settings are used.",
              "Open the “{tabJobs}” tab.",
              "Drag a .drt, .fcpxml, .srt, .ttml or .transcript.json file into the box and let go. You can also press “{pickSubtitles}”.",
              "The list under the box shows the progress and the result.",
            ],
            notes: [
              "The write-again button next to a video's name in the list makes new files with a different name. The old files are kept.",
              "The original files are kept. The new files get -2 or a similar number added to their name.",
              "The new file uses the type chosen in “{subtitleFormat}”, whatever file you added.",
              "The .transcript.json file with the same name must be in the same folder.",
              "When you are using a profile, changes you have not saved to it are not used.",
            ],
          },
        ],
      },
      settings: {
        heading: "Settings and updates",
        parts: [
          {
            heading: "The settings",
            intro: "The “{tabSettings}” tab shows the version of the app. Your choices stay for next time.",
            steps: [
              "Open the “{tabSettings}” tab.",
              "Under “{uiLanguageLabel}”, choose a language. The screen changes at once.",
              "Under “{uiThemeLabel}”, choose “{uiThemeLight}” or “{uiThemeDark}”. “{uiThemeSystem}” follows your computer.",
            ],
            notes: [
              "At the bottom of the tab, “{licensesOpen}” lists the libraries in the app and their licenses.",
            ],
          },
          {
            heading: "Update the app",
            intro:
              "When a new version is out, the app shows it at the top of the window. Nothing changes until you press the button.",
            steps: [
              "To look for one yourself, press “{updateCheck}” on the “{tabSettings}” tab.",
              "Read what is new, then press “{updateInstall}”.",
              "The app downloads the update and restarts.",
            ],
            notes: [
              "You cannot update while videos are being made or models are downloading. Wait until they are finished.",
              "Checking and downloading need an internet connection.",
            ],
          },
        ],
      },
      trouble: {
        heading: "If something goes wrong",
        parts: [
          {
            heading: "It says a model is missing",
            intro: "The app cannot start making subtitles without the models it needs.",
            steps: [
              "Open the “{tabModels}” tab.",
              "Press “{downloadAllLabel}”.",
              "When that is done, add the video again.",
            ],
            notes: [],
          },
          {
            heading: "Subtitles for 2 or more speakers cannot start",
            intro:
              "Automatic detection and 2 or more speakers need the Nemotron 3.5 encoder and MossFormer2, and at least 8 GB of RAM.",
            steps: [],
            notes: [
              "On the “{tabModels}” tab, download the missing models. If they do not fit this computer, press “{runWithOneSpeaker}”.",
              "With 1 speaker, these models are not needed, and everyone gets the same colours.",
            ],
          },
          {
            heading: "It says no face was found",
            intro:
              "When no face is found, the subtitle goes at the bottom of the video. If you choose a place that does not use faces, subtitles always stay in one place.",
            steps: [
              "Open the “{tabStyle}” tab.",
              "Under the place for subtitles, choose the top or bottom of the video.",
              "Press the write-again button next to the video's name.",
            ],
            notes: [
              "If it is not in the list, add the subtitle file. If you are using a profile, overwrite the profile first.",
              "Avatar faces need a different model. Put its file in a model folder, then choose it in “{faceModel}”.",
            ],
          },
          {
            heading: "You want to start over",
            intro: "If a model may be damaged, you can get it again.",
            steps: ["Open the “{tabModels}” tab.", "Press “{resetModels}”.", "Read the message, then go on."],
            notes: [
              "Only what this app downloaded is deleted. Folders you added are kept.",
              "You cannot do this while subtitles are being made or a download is running.",
              "After deleting, the download of the missing models starts by itself.",
            ],
          },
          {
            heading: "Your video editor cannot find the video",
            intro:
              "A .drt or FCPXML file holds the video's place and name. If you move or rename the video, your video editor cannot find it.",
            steps: [
              "Put the video back in its old place, with its old name.",
              "Open the .drt/.fcpxml file again.",
            ],
            notes: [],
          },
        ],
      },
    },
    imageFolder: "en",
    resolveFigures: {
      menu: "Choose File, then Import, then Timeline. 1 is File, 2 is Import, 3 is Timeline.",
      dialog: "The Import Timeline screen. 1 is Automatically set project setting, 2 is the Import button.",
      timeline: "The timeline after importing. 1 is the Text+ clips on V2.",
      text: "The Inspector with a clip selected. 1 is the Text tab, 2 is where you change the words.",
      shading: "The Shading tab of the Inspector. 1 is Shading, 2 is the numbers of Select Element.",
    },
    images: {
      models: "The Models tab. 1 is the tab, 2 is the download button, 3 is the bar for each model.",
      transcribe:
        "The Transcribe tab. 1 is the tab, 2 is the number of speakers, 3 is the file type, 4 is the box for videos.",
      look: "The caption look tab. 1 is the tab, 2 is where the subtitles go, 3 is a slider, 4 is a person's colours.",
      font: "The open font list. 1 shows the font's name, 2 is where you type, 3 is the font to choose.",
      profiles: "The profiles area. 1 is where you type a name, 2 is the save button, 3 is the load button.",
      rewrite:
        "After adding a subtitle file. 2 is the tab, 3 is the box for files, 4 is the list of results.",
    },
  },
};
