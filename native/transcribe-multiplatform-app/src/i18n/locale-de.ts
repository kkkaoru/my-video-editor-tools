// Runs with bun: German UI text.
import type { Messages } from "./messages.ts";

export const deMessages: Messages = {
  appName: "Transcribe Captions",
  tabJobs: "Transkribieren",
  tabStyle: "Aussehen und Position der Untertitel",
  tabHelp: "Anleitung",
  tabModels: "Modelle",
  noticeSpeakersOnCpu:
    "Das Unterscheiden der Sprecher läuft auf der CPU und dauert länger. Lade unter Modelle die GeForce-Daten, dann geht es schneller.",
  noticeCpuOnly:
    "Es wurde keine GPU gefunden, die diese Version nutzen kann. Alles läuft auf der CPU und dauert länger.",
  dropTitle: "Videos oder Untertiteldateien hier ablegen",
  dropHint:
    "Aus Videos entstehen Untertiteldateien für Videoschnittprogramme, gespeichert neben dem jeweiligen Video.",
  dropHover: "Loslassen, um die Dateien hinzuzufügen",
  pickVideos: "Videos auswählen",
  pickSubtitles: "Untertiteldateien auswählen",
  subtitleFilterName: "Untertiteldateien",
  dropProfileHint:
    "Abgelegte Untertiteldateien werden mit dem gespeicherten Profil {name} als neue Dateien neu geschrieben.",
  dropCurrentHint:
    "Abgelegte Untertiteldateien werden mit den aktuellen Untertiteleinstellungen als neue Dateien neu geschrieben.",
  speakerCountHelpLabel: "Warum die Anzahl der Sprecher angeben?",
  speakerCountHelp:
    "Die Sprecherfarben hängen davon ab, dass Stimmen unterschieden werden. Mit der Anzahl verschmelzen ähnliche Stimmen seltener, und eine Stimme wird seltener aufgeteilt. Bei 1 Sprecher entfällt die Sprechertrennung; das geht schneller und braucht kein Sprechermodell.",
  profilesHeading: "Profile",
  profilesHint:
    "Aussehen und Position der Untertitel unter einem Namen speichern und später wiederherstellen. Modelle und die gesprochene Sprache gehören nicht zum Profil.",
  profileName: "Profilname",
  profileSave: "Als neues Profil speichern",
  profileEmpty: "Noch keine Profile gespeichert.",
  profileSelected: "Ausgewählt",
  profileChanged: "Ungespeicherte Änderungen",
  profileLoad: "{name} laden",
  profileOverwrite: "{name} überschreiben",
  profileOverwriteTitle: "{name} überschreiben?",
  profileOverwriteDescription:
    "Das gespeicherte Aussehen wird durch die aktuellen Einstellungen ersetzt. Das lässt sich nicht rückgängig machen.",
  profileDelete: "{name} löschen",
  profileDeleteTitle: "{name} löschen?",
  profileDeleteDescription:
    "Das Profil wird entfernt. Die aktuellen Einstellungen und vorhandene Untertiteldateien bleiben unverändert. Das lässt sich nicht rückgängig machen.",
  profileExport: "{name} exportieren",
  profileImport: "Aus einer Datei importieren",
  profileFileFilter: "Profile",
  resetLook: "Ursprüngliches Aussehen und Position wiederherstellen",
  resetLookTitle: "Ursprüngliches Aussehen und Position wiederherstellen?",
  resetLookCurrent:
    "Das aktuelle Aussehen und die Position gehen zurück auf die ersten Einstellungen der App. Andere Profile, die Anzahl der Sprecher, Modelle und die Sprache ändern sich nicht. Das lässt sich nicht rückgängig machen.",
  resetLookProfile:
    "Aussehen und Position des Profils {name} gehen zurück auf die ersten Einstellungen der App. Andere Profile, die Anzahl der Sprecher, Modelle und die Sprache ändern sich nicht. Das lässt sich nicht rückgängig machen.",
  resetLookConfirm: "Wiederherstellen",
  dialogCancel: "Abbrechen",
  fontFamily: "Schriftfamilie",
  fontSearch: "Schriften durchsuchen",
  fontHint: "Gilt für alle Sprecher. Tippe, um die Liste einzugrenzen.",
  fontLoading: "Installierte Schriften werden gelesen",
  fontNoMatch: "Keine installierte Schrift passt",
  fontNotInstalled:
    "{font} ist auf diesem Computer nicht installiert. Die Vorschau nutzt einen Ersatz, die Untertiteldateien nennen weiter diesen Namen.",
  downloadAllLabel: "Fehlendes herunterladen",
  downloadAll: "{label}: {count} ({size})",
  resetModels: "Modelle zurücksetzen und neu herunterladen",
  resetModelsTitle: "Modelle zurücksetzen und neu herunterladen?",
  resetModelsDescription:
    "Alles, was diese App in {path} heruntergeladen hat, wird gelöscht; danach werden die Modelle, die dieser Computer braucht, neu heruntergeladen. Von dir hinzugefügte Ordner bleiben unberührt. Das lässt sich nicht rückgängig machen.",
  resetModelsConfirm: "Löschen und neu herunterladen",
  videoFilterName: "Videos",
  readinessMissing:
    "Für {roles} ist kein Modell verfügbar. Lade unter „Modelle“ eines herunter oder füge einen Ordner hinzu, der bereits eines enthält.",
  roleTranscribe: "Transkription",
  roleDiarize: "Sprechertrennung",
  roleFace: "Gesichtserkennung",
  roleRuntime: "Laufzeitumgebung",
  jobQueued: "In der Warteschlange",
  jobCancelled: "Abgebrochen",
  jobCancel: "{name} abbrechen",
  jobRelayout: "{name} mit der aktuellen Position und den aktuellen Farben neu schreiben",
  jobDone: "Fertig: {cues} Untertitel, {speakers} Sprecher",
  jobDoneOneSpeaker: "Fertig: {cues} Untertitel, 1 Sprecher",
  jobFaceless: "Untertitel an Stellen ohne erkanntes Gesicht wurden unten im Video platziert.",
  jobFontMissing:
    "Die gewählte Schrift fehlt auf diesem Computer, darum ist die Breite der Untertitel geschätzt.",
  jobNoPercent: "Dieses Modell meldet keinen Prozentwert",
  speakerCount: "Anzahl der Sprecher",
  subtitleFormat: "Format der Untertiteldatei",
  speakerCountAuto: "Automatisch erkennen",
  speakerCountValue: "{count} Sprecher",
  speakerCountOne: "1 Sprecher",
  jobFewerSpeakers:
    "{requested} Sprecher wurden angegeben, aber nur {found} gefunden. Das kann bei kurzen Aufnahmen oder ähnlichen Stimmen passieren.",
  jobStep: "Schritt {number} von {total}: {stage}",
  jobEstimating: "Restzeit wird berechnet",
  jobRemaining: "Noch etwa {minutes} Min.",
  jobRemainingUnderMinute: "Noch weniger als eine Minute",
  stepStates: { done: "fertig", current: "läuft", pending: "wartet" },
  outputPurposes: {
    ttml: "Gestaltete Untertitel (Farbe, Kontur, Position)",
    srt: "Untertitel mit Wörtern und Zeiten (Farben vielleicht nicht sichtbar, keine Ränder und Positionen)",
    fcpxml: "Timeline mit Titeln",
    json: "Transkriptdaten zum erneuten Schreiben",
  },
  jobProgressLabel: "Fortschritt von {name}",
  jobOutputs: "Geschriebene Dateien",
  stages: {
    convert_model: "Modell wird umgewandelt",
    extract_audio: "Audio wird extrahiert",
    transcribe: "Transkription läuft",
    diarize: "Sprecher werden getrennt",
    faces: "Gesichter werden erkannt",
    export: "Untertiteldateien werden geschrieben",
  },
  positionHeading: "Position der Untertitel",
  positionLabel: "Untertitel platzieren",
  anchorFaceAbove: "Über dem Gesicht",
  anchorFaceBelow: "Unter dem Gesicht",
  anchorVideoTop: "Oben im Video",
  anchorVideoBottom: "Unten im Video",
  offsetX: "Feinabstimmung links und rechts (Prozent der Breite)",
  offsetY: "Feinabstimmung oben und unten (Prozent der Höhe)",
  safeZoneHint:
    "Hier legst du fest, wie breit die Ränder links und rechts sind, auf denen keine Untertitel stehen. Die Vorschau zeigt sie als dunkle Streifen.",
  safeLeft: "Sicherheitszone links (Prozent der Breite)",
  safeRight: "Sicherheitszone rechts (Prozent der Breite)",
  faceModel: "Modell für die Gesichtserkennung",
  speechLanguage: "Gesprochene Sprache",
  languageAuto: "Automatisch erkennen",
  sizeHeading: "Größe der Untertitel",
  sizeHint:
    "Die Größen passen sich automatisch der Auflösung jedes Videos an. Die Werte sind Anteile am Video.",
  fontSize: "Schriftgröße (Prozent der kurzen Kante)",
  captionWidth: "Längste Zeile (Prozent der Breite)",
  outlineWidth: "Konturstärke (Prozent der Schrift)",
  captionLineHeight: "Zeilenabstand (Vielfaches der Textgröße)",
  rimWidth: "Schwarzer Rand außerhalb der Kontur (Prozent des Texts)",
  unknownSpeaker: "Unbekannter Sprecher",
  contrastWarning:
    "{name}: Der Kontrast zwischen Füllung und Kontur beträgt {ratio}:1 und liegt unter dem WCAG-AA-Mindestwert von 4,5:1. Wähle Farben mit größerem Helligkeitsunterschied.",
  previewHeading: "Vorschau",
  previewFrameLabel: "Vorschaugröße",
  previewDescription: "Vorschau-Untertitel bei {x} % von links und {y} % von oben im {size}-Bild.",
  previewFontPixels: "Textgröße bei dieser Auflösung: {px} px.",
  previewSample: "Untertitel mit Kontur wechseln die Farbe je Sprecher",
  previewLandscape: "1920×1080",
  previewVertical: "Hochformat 1080×1920",
  previewUhd: "4K",
  speakersHeading: "Sprecherfarben (bis zu {count} Sprecher)",
  speakersHint:
    "Jeder Sprecher erhält automatisch eine Füll- und eine Konturfarbe. Hier kannst du sie ändern.",
  speakerName: "Sprecher {number}",
  fillLabel: "Füllung",
  outlineLabel: "Kontur",
  addSpeaker: "Sprecher hinzufügen",
  removeSpeaker: "Sprecher entfernen",
  resetPalette: "Ursprüngliche Farben wiederherstellen",
  modelsHeading: "Modelle",
  noRecommendedSpeakerModel:
    "Für diesen Computer wird kein Modell zur Sprechertrennung empfohlen. Stelle die Anzahl der Sprecher auf 1, oder lade aus der Liste unten ein kleines Modell und wähle es aus.",
  modelsHint:
    "Die für diesen Computer geeigneten Modelle sind als empfohlen markiert. Bereits heruntergeladene Modelle werden gefunden und wiederverwendet.",
  addModelFolder: "Ordner mit vorhandenen Modellen hinzufügen",
  modelFolders: "Durchsuchte Ordner",
  badgeRecommended: "Hier empfohlen",
  badgePresent: "Verfügbar",
  useModel: "{name} verwenden",
  inUseModel: "In Verwendung: {name}",
  download: "{name} herunterladen ({size})",
  downloadCancel: "Download von {name} abbrechen",
  downloadProgress: "{done} von {total}",
  downloadProgressLabel: "Download von {name}",
  deleteModel: "Heruntergeladene Kopie von {name} löschen",
  sourceLabel: "Quelle: {url}",
  locationLabel: "Speicherort: {path}",
  loadFailed: "Etwas ist schiefgelaufen: {detail}",
  errors: {
    no_video_stream: "Diese Datei enthält kein Video. Wähle eine Videodatei aus.",
    no_audio_stream: "Dieses Video hat keinen Ton, daher gibt es nichts zu transkribieren.",
    unreadable_size:
      "Die Größe dieses Videos konnte nicht gelesen werden. Die Datei ist möglicherweise beschädigt.",
    unreadable_duration:
      "Die Länge dieses Videos konnte nicht gelesen werden. Die Datei ist möglicherweise beschädigt.",
    no_words: "Im Audio wurden keine Wörter gefunden.",
    model_missing: "{model} ist nicht installiert. Lade es unter „Modelle“ herunter.",
    no_model_for_role:
      "Für {role} ist kein Modell installiert. Stelle die Anzahl der Sprecher auf 1, oder lade unter Modelle ein kleines Modell und wähle es aus.",
    needs_nvidia: "{model} benötigt eine NVIDIA GPU. Wähle unter „Modelle“ ein anderes Modell.",
    needs_memory:
      "{model} benötigt {required} GB Arbeitsspeicher; dieser Computer hat {available} GB. Wähle ein kleineres Modell.",
    too_long_for_model:
      "{model} verarbeitet Aufnahmen bis {limit} Minuten. Dieses Video ist länger. Wähle unter Modelle Nemotron 3 Diarization.",
    transcript_missing:
      "Neben {file} liegt keine Transkription, daher kann die Datei nicht neu geschrieben werden. Lege die gleichnamige .transcript.json-Datei in denselben Ordner.",
    profile_exists:
      "Ein Profil namens {name} gibt es bereits. Wähle einen anderen Namen oder überschreibe es in der Liste.",
    profile_missing: "Das Profil {name} gibt es nicht mehr.",
    models_busy:
      "Während eine Aufgabe oder ein Download läuft, können Modelle nicht zurückgesetzt werden. Warte das Ende ab oder brich ab und versuche es erneut.",
    models_missing:
      "Die Videos wurden nicht hinzugefügt, weil für Folgendes kein Modell verfügbar ist: {roles}. Lade sie unter Modelle herunter oder füge einen Ordner hinzu, der sie enthält.",
    no_supported_files:
      "Diese Datei geht nicht. Wähle ein Video (.mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts) oder eine Untertiteldatei (.fcpxml .srt .ttml .transcript.json).",
    profile_invalid:
      "{file} lässt sich nicht als Profil lesen. Wähle eine Datei, die diese App exportiert hat.",
    internal: "{detail}",
    cancelled: "Abgebrochen",
  },
  modelNotes: {
    "whisper-large-v3-turbo":
      "Transkription. Dasselbe Modell läuft auf dem Mac (Metal) und auf Windows (Vulkan).",
    "nemotron-3.5-transcribe-iq4":
      "Sprechertrennung. Für Macs mit 32 GB oder mehr, GPUs mit 24 GB oder mehr und CPUs.",
    "nemotron-3.5-transcribe-nvfp4": "Sprechertrennung. Für Blackwell-GPUs wie die GeForce-RTX-50-Serie.",
    "nemotron-3.5-transcribe-q2m":
      "Sprechertrennung. Für GPUs mit 16 GB; jede Aufnahme kann etwa 7 Minuten lang sein.",
    "nemotron-3.5-transcribe-q2l": "Sprechertrennung. Für Macs mit 24 GB.",
    "nemotron-3.5-transcribe-q8":
      "Sprechertrennung. Höchste Qualität, für Macs mit 64 GB und GPUs mit 48 GB.",
    "nemotron-3-diarization":
      "Nur Sprechertrennung, 107 MB. Bis zu 8 Sprecher; läuft mit wenig Arbeitsspeicher.",
    yunet: "Echte Gesichter. Im Lieferumfang der App; liefert auf Mac und Windows dasselbe Ergebnis.",
    "avatar-face-yolo":
      "Avatar-Gesichter. Funktioniert, sobald die exportierte ONNX-Datei in einem Modellordner liegt.",
    "geforce-runtime":
      "CUDA-Version des Werkzeugs zur Sprechertrennung. Ohne sie läuft die Sprechertrennung auf der CPU.",
  },
  help: {
    version: "Version {version}",
    intro:
      "Diese App macht aus dem, was in einem Video gesagt wird, Untertitel. Jede Person bekommt eine eigene Farbe. Die Untertitel kannst du in einem Videoschnittprogramm nutzen.",
    sections: {
      start: {
        heading: "Der Anfang",
        parts: [
          {
            heading: "Die App installieren",
            intro: "Lade die Datei, die zu deinem Computer passt, und installiere sie.",
            steps: [
              "Wähle aus der Liste unten die Datei für deinen Computer.",
              "Auf einem Mac öffnest du die Zip-Datei und ziehst die App in den Ordner „Programme“.",
              "Unter Windows öffnest du die Exe-Datei und folgst den Schritten.",
            ],
            notes: [
              "Mac: Transcribe-Captions-macos-arm64.zip",
              "Windows mit GeForce: Transcribe-Captions-windows-geforce-x64-setup.exe",
              "Windows mit Radeon: Transcribe-Captions-windows-radeon-x64-setup.exe",
              "Windows mit keiner von beiden: Jede der Exe-Dateien geht. Sie läuft nur auf der CPU und dauert länger.",
              "Wenn der Mac dich stoppt: Öffne Systemeinstellungen > Datenschutz & Sicherheit und drücke „Dennoch öffnen“.",
              "Wenn Windows dich stoppt: Drücke „Weitere Informationen“ und dann „Trotzdem ausführen“.",
            ],
          },
          {
            heading: "Modelle herunterladen",
            intro:
              "Modelle sind die Daten, mit denen die App Sprache in Text verwandelt. Du brauchst sie nur einmal zu holen.",
            steps: [
              "Öffne den Tab „{tabModels}“.",
              "Drücke „{downloadAllLabel}“.",
              "Warte, bis der Balken jedes Modells am Ende ist.",
            ],
            notes: [
              "Ein fertiges Modell zeigt „{badgePresent}“.",
              "Fehlt nichts, gibt es diesen Knopf nicht.",
              "Du brauchst Internet. Die Dateien sind groß, das dauert etwas.",
              "Bleibt ein Download stehen, drücke den Knopf noch einmal. Er läuft dann an derselben Stelle weiter.",
            ],
          },
          {
            heading: "Vorhandene Modelle nutzen",
            intro: "Hast du Modelle schon einmal geladen, musst du sie nicht noch einmal laden.",
            steps: [
              "Öffne den Tab „{tabModels}“.",
              "Drücke „{addModelFolder}“.",
              "Wähle den Ordner mit den Modellen.",
            ],
            notes: ["Gefundene Modelle zeigen „{badgePresent}“."],
          },
        ],
      },
      transcribe: {
        heading: "Untertitel aus einem Video machen",
        parts: [
          {
            heading: "Ein Video hinzufügen",
            intro: "Sobald du ein Video hinzufügst, beginnt die App mit den Untertiteln.",
            steps: [
              "Öffne den Tab „{tabJobs}“.",
              "„{speakerCount}“ steht am Anfang auf 1. Sprechen mehr Menschen, wähle ihre Anzahl. Bist du nicht sicher, wähle „{speakerCountAuto}“.",
              "Wähle bei „{subtitleFormat}“ die Dateiart. Am Anfang steht dort FCPXML. Darunter steht, wofür sie gut ist.",
              "Ziehe das Video in das Feld und lass es los. Ziehen heißt: gedrückt halten und bewegen. Du kannst auch „{pickVideos}“ drücken.",
            ],
            notes: [
              "Das „?“ neben der Anzahl erklärt, warum sie hilft.",
              "Spricht nur eine Person, lass 1 stehen. Das geht schneller.",
              "Diese Videos gehen: .mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts. Untertiteldateien: .fcpxml .srt .ttml .transcript.json. Bei anderen Dateien sagt die App, dass sie nicht gehen.",
              "Mehrere Videos werden eines nach dem anderen bearbeitet.",
              "Beim ersten Mal bringt die App außerdem ein Modell in eine Form, die sie nutzen kann.",
              "Stimmt die Sprache der Untertitel nicht, wähle sie im Tab „{tabStyle}“ bei „{speechLanguage}“.",
            ],
          },
          {
            heading: "Den Fortschritt sehen",
            intro: "Nach dem Hinzufügen erscheint unter dem Feld eine Liste.",
            steps: [],
            notes: [
              "Sie zeigt den aktuellen Schritt und wie viele Schritte es gibt.",
              "Der Balken zeigt den Fortschritt der ganzen Arbeit.",
              "Sie zeigt auch ungefähr, wie lange es noch dauert.",
              "Zum Anhalten drücke den Knopf neben dem Namen des Videos.",
            ],
          },
          {
            heading: "Die fertigen Dateien",
            intro:
              "Im Ordner des Videos erscheinen zwei Dateien: die Untertiteldatei in der gewählten Art und eine .transcript.json-Datei. Ältere Dateien bleiben. Ist der Name schon vergeben, bekommen die neuen Dateien ein -2 im Namen.",
            steps: [],
            notes: [
              ".fcpxml: Untertitel und Video zusammen, für Videoschnittprogramme.",
              ".ttml: Untertitel mit Farben und Positionen.",
              ".srt: Untertitel mit Wörtern und Zeiten. Farben werden vielleicht nicht gezeigt. Ränder und Positionen fehlen.",
              ".transcript.json: die Daten, um Untertitel neu zu schreiben. Bitte behalten.",
            ],
          },
        ],
      },
      editor: {
        heading: "Im Videoschnittprogramm öffnen",
        parts: [
          {
            heading: "Eine FCPXML-Datei holen",
            intro:
              "Eine FCPXML-Datei holt Video und Untertitel auf einmal, mit Farben, Rändern und Positionen. Darum ist sie am Anfang eingestellt.",
            steps: [
              "Öffne ein Projekt in deinem Videoschnittprogramm. Ein Projekt enthält deine ganze Schnittarbeit.",
              "Hole die Timeline über das Menü (zum Beispiel File > Import > Timeline).",
              "Wähle die .fcpxml-Datei, die die App gemacht hat.",
              "Video und Untertitel liegen nun auf einer Timeline. Dort liegt das Video der Zeit nach geordnet.",
            ],
            notes: [
              "Jeder Untertitel besteht aus 2 oder 3 Ebenen übereinander. Änderst du die Wörter, ändere alle Ebenen.",
            ],
          },
          {
            heading: "Wann die anderen Arten nützen",
            intro: "Eine andere Art wählst du bei „{subtitleFormat}“, bevor du das Video hinzufügst.",
            steps: [],
            notes: [
              "Eine .ttml-Datei holst du im Media Pool mit Import Subtitle. Im Media Pool liegen deine Videos und Untertitel.",
              "Die .srt-Datei nimmst du, um Wörter oder Zeiten im Videoschnittprogramm zu ändern. Schreibt diese App die Untertitel neu, fehlen deine Änderungen.",
              "Nutze FCPXML und TTML nicht auf derselben Timeline. Sonst sind die Untertitel doppelt zu sehen.",
            ],
          },
        ],
      },
      look: {
        heading: "Aussehen und Platz der Untertitel ändern",
        parts: [
          {
            heading: "Aussehen und Platz ändern",
            intro:
              "Änderungen siehst du sofort in der Vorschau. Sie werden von selbst gespeichert. Die Vorschau zeigt, wie die Untertitel aussehen werden.",
            steps: [
              "Öffne den Tab „{tabStyle}“.",
              "Wähle bei „{positionHeading}“: „{anchorFaceAbove}“, „{anchorFaceBelow}“, „{anchorVideoTop}“ oder „{anchorVideoBottom}“.",
              "Stelle mit den Reglern Textgröße und Randbreite ein. Ein Regler ist ein Griff, den du nach links und rechts schiebst. Du kannst auch eine Zahl eintippen.",
              "Wähle für jede Person die Textfarbe und die Randfarbe.",
            ],
            notes: [
              "Die Vorschau steht auf einem breiten Bildschirm rechts und auf einem schmalen oben. Sie bleibt beim Scrollen sichtbar.",
              "Sollen Untertitel nicht an den Rändern links und rechts stehen, stelle unter „{positionHeading}“ die Sicherheitszonen ein. Die dunklen Streifen in der Vorschau zeigen sie.",
            ],
          },
          {
            heading: "Eine Schrift wählen",
            intro:
              "Die Schrift bestimmt die Form der Buchstaben. Du wählst aus den Schriften auf diesem Computer. Alle Personen bekommen dieselbe Schrift.",
            steps: [
              "Drücke unter „{fontFamily}“ auf die Stelle mit dem Namen der Schrift.",
              "Tippe einen Teil des Namens ein. Die Liste wird kürzer.",
              "Drücke die gewünschte Schrift.",
            ],
            notes: [],
          },
          {
            heading: "Ein Profil speichern und laden",
            intro: "Ein Profil sind Einstellungen für Aussehen und Platz, unter einem Namen gespeichert.",
            steps: [
              "Gib einen Namen ein.",
              "Drücke „{profileSave}“.",
              "Um es später zu nutzen, drücke in der Liste den Knopf zum Laden.",
            ],
            notes: [
              "Vor dem Überschreiben oder Löschen fragt die App nach.",
              "Modelle und die gesprochene Sprache gehören nicht zum Profil.",
            ],
          },
          {
            heading: "Ein Profil auf einem anderen Computer nutzen",
            intro: "Exportierst du ein Profil in eine Datei, kannst du es auf einem anderen Computer nutzen.",
            steps: [
              "Drücke in der Liste den Knopf zum Exportieren und wähle, wo die Datei hinkommt.",
              "Kopiere die Datei auf den anderen Computer.",
              "Drücke dort „{profileImport}“ und wähle die Datei.",
            ],
            notes: [
              "Ein importiertes Profil erscheint in der Liste.",
              "Ist der Name schon vergeben, kommt es unter einem anderen Namen dazu. Nichts wird ersetzt.",
            ],
          },
          {
            heading: "Zum ersten Aussehen zurück",
            intro: "Du kannst Aussehen und Position auf die ersten Einstellungen der App zurücksetzen.",
            steps: [
              "Öffne den Tab „{tabStyle}“.",
              "Drücke „{resetLook}“.",
              "Lies den Hinweis und mache dann weiter.",
            ],
            notes: [
              "Nutzt du ein Profil, geht nur dieses Profil zurück.",
              "Die Anzahl der Sprecher, Modelle, die Sprache und andere Profile ändern sich nicht.",
            ],
          },
        ],
      },
      rewrite: {
        heading: "Fertigen Untertiteln ein neues Aussehen geben",
        parts: [
          {
            heading: "Eine Untertiteldatei hinzufügen",
            intro:
              "Du kannst das Aussehen ändern, ohne die Untertitel neu aus dem Ton zu machen. Meist geht das schnell. Muss die App Gesichter suchen, dauert es länger.",
            steps: [
              "Lade das gewünschte Profil. Ist keines geladen, gelten die aktuellen Einstellungen.",
              "Öffne den Tab „{tabJobs}“.",
              "Ziehe eine .fcpxml-, .srt-, .ttml- oder .transcript.json-Datei in das Feld und lass sie los. Du kannst auch „{pickSubtitles}“ drücken.",
              "Die Liste unter dem Feld zeigt Fortschritt und Ergebnis.",
            ],
            notes: [
              "Der Knopf zum Neuschreiben neben dem Namen eines Videos in der Liste macht neue Dateien mit einem anderen Namen. Die alten Dateien bleiben erhalten.",
              "Die alten Dateien bleiben. Die neuen bekommen ein -2 oder eine ähnliche Zahl im Namen.",
              "Die Art der neuen Datei richtet sich nach „{subtitleFormat}“, nicht nach der hinzugefügten Datei.",
              "Die .transcript.json-Datei mit demselben Namen muss im selben Ordner liegen.",
              "Nutzt du ein Profil, werden Änderungen nicht verwendet, die du nicht darin gespeichert hast.",
            ],
          },
        ],
      },
      trouble: {
        heading: "Wenn etwas nicht klappt",
        parts: [
          {
            heading: "Es fehlt ein Modell",
            intro: "Ohne die nötigen Modelle kann die App keine Untertitel machen.",
            steps: [
              "Öffne den Tab „{tabModels}“.",
              "Drücke „{downloadAllLabel}“.",
              "Füge danach das Video noch einmal hinzu.",
            ],
            notes: [],
          },
          {
            heading: "Es fehlt ein Modell zum Unterscheiden der Sprecher",
            intro: "Für manche Computer gibt es kein empfohlenes Modell. Mache eines von beiden.",
            steps: [],
            notes: [
              "Spricht nur eine Person im Video, wähle bei „{speakerCount}“ die 1. Dieses Modell brauchst du dann nicht.",
              "Lade im Tab „{tabModels}“ ein kleines Modell und drücke seinen Knopf zum Verwenden.",
            ],
          },
          {
            heading: "Es wurde kein Gesicht gefunden",
            intro:
              "Ohne Gesicht steht der Untertitel unten im Video. Wählst du einen Platz ohne Gesicht, bleiben die Untertitel immer am selben Ort.",
            steps: [
              "Öffne den Tab „{tabStyle}“.",
              "Wähle beim Platz der Untertitel oben oder unten im Video.",
              "Drücke den Knopf zum Neuschreiben neben dem Namen des Videos.",
            ],
            notes: [
              "Steht es nicht in der Liste, füge die Untertiteldatei hinzu. Nutzt du ein Profil, überschreibe es vorher.",
              "Avatar-Gesichter brauchen ein anderes Modell. Lege die Datei in einen Modellordner und wähle sie bei „{faceModel}“.",
            ],
          },
          {
            heading: "Du willst neu anfangen",
            intro: "Wenn ein Modell vielleicht kaputt ist, kannst du es neu holen.",
            steps: [
              "Öffne den Tab „{tabModels}“.",
              "Drücke „{resetModels}“.",
              "Lies den Hinweis und mache dann weiter.",
            ],
            notes: [
              "Gelöscht wird nur, was diese App geladen hat. Ordner, die du hinzugefügt hast, bleiben.",
              "Das geht nicht, solange Untertitel entstehen oder etwas geladen wird.",
              "Nach dem Löschen startet der Download der fehlenden Modelle von selbst.",
            ],
          },
          {
            heading: "Das Videoschnittprogramm findet das Video nicht",
            intro:
              "In einer FCPXML-Datei stehen Ort und Name des Videos. Verschiebst du das Video oder benennst es um, findet das Schnittprogramm es nicht.",
            steps: [
              "Lege das Video zurück an seinen alten Ort, mit seinem alten Namen.",
              "Öffne die .fcpxml-Datei noch einmal.",
            ],
            notes: [],
          },
        ],
      },
    },
    imageFolder: "de",
    images: {
      models: "Der Tab Modelle. 1 ist der Tab, 2 der Knopf zum Herunterladen, 3 der Balken jedes Modells.",
      transcribe:
        "Der Tab für Untertitel. 1 ist der Tab, 2 die Anzahl der Sprecher, 3 die Dateiart, 4 das Feld für Videos.",
      look: "Der Tab für Aussehen und Position. 1 ist der Tab, 2 der Platz der Untertitel, 3 ein Regler, 4 die Farben einer Person.",
      font: "Die offene Schriftenliste. 1 zeigt den Namen der Schrift, 2 ist das Eingabefeld, 3 die Schrift zum Wählen.",
      profiles:
        "Der Bereich Profile. 1 ist das Feld für den Namen, 2 der Knopf zum Speichern, 3 der Knopf zum Laden.",
      rewrite:
        "Nach dem Hinzufügen einer Untertiteldatei. 2 ist der Tab, 3 das Feld für Dateien, 4 die Liste der Ergebnisse.",
    },
  },
};
