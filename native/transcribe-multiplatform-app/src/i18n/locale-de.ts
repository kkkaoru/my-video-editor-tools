// Runs with bun: German UI text.
import type { Messages } from "./messages.ts";

export const deMessages: Messages = {
  appName: "Transcribe Captions",
  tabJobs: "Transkribieren",
  tabStyle: "Aussehen und Position der Untertitel",
  tabSettings: "Einstellungen",
  uiLanguageLabel: "Sprache der Oberfläche",
  uiLanguageSystem: "Wie das System",
  uiThemeLabel: "Darstellung",
  uiThemeSystem: "Wie das System",
  uiThemeLight: "Hell",
  uiThemeDark: "Dunkel",
  updateCheck: "Nach Updates suchen",
  updateChecking: "Es wird nach Updates gesucht",
  updateUpToDate: "Das ist die neueste Version.",
  updateCheckFailed: "Die Suche nach Updates hat nicht geklappt: {detail}",
  updateAvailable: "Version {version} ist verfügbar",
  updateInstall: "Aktualisieren und neu starten",
  updateRetry: "Update noch einmal versuchen",
  updateBusy: "Du kannst aktualisieren, wenn die aktuellen Videos und Modell-Downloads fertig sind.",
  updateInstalling: "Das Update wird geladen. Danach startet die App neu.",
  updateProgressLabel: "Update wird geladen",
  updateInstallFailed: "Das Update wurde nicht fertig: {detail}",
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
  speakerCountOnlyOne:
    "Ohne ein Nemotron-3.5-Modell geht nur 1 Sprecher. Lade unter „{tabModels}“ eines, dann kannst du mehr wählen.",
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
    drt: "Text+-Untertitel für DaVinci Resolve mit Farben, Konturen, äußeren Rändern und Schatten",
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
  captionLineHeight: "Zeilenabstand (Vielfaches der Textgröße)",
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
    "Wähle einen Sprecher, um seine Farben und Stärken zu ändern. Die Vorschau zeigt seine Untertitel. Stärke und Position sind Prozentwerte der Schriftgröße.",
  speakerName: "Sprecher {number}",
  colorColumn: "Farbe",
  fillWidth: "Stärke der Füllung (Prozent der Schriftgröße)",
  outlineWidth: "Stärke der Kontur (Prozent der Schriftgröße)",
  rimWidth: "Stärke des äußeren Rands (Prozent der Schriftgröße)",
  layerWidth: "Stärke (Prozent der Schriftgröße)",
  colorTransparent: "Transparent",
  colorCustom: "Eigene Farbe",
  colorNames: {
    white: "Weiß",
    black: "Schwarz",
    red: "Rot",
    orange: "Orange",
    yellow: "Gelb",
    green: "Grün",
    blue: "Blau",
    purple: "Lila",
  },
  fillLabel: "Füllung",
  outlineLabel: "Kontur",
  rimLabel: "Äußerer Rand",
  formatLimits: {
    fcpxml: "FCPXML zeigt keinen Schatten. Wähle dafür DRT.",
    ttml: "TTML zeigt weder äußeren Rand noch Schatten. Wähle dafür DRT.",
    srt: "SRT behält nur die Textfarbe, ohne Kontur, äußeren Rand und Schatten. Wähle dafür DRT.",
  },
  shadowLabel: "Schatten",
  shadowColor: "Schattenfarbe",
  shadowX: "Schatten links/rechts (Prozent der Schriftgröße)",
  shadowXShort: "Schatten links/rechts",
  shadowY: "Schatten oben/unten (Prozent der Schriftgröße; minus ist unten)",
  shadowYShort: "Schatten oben/unten",
  shadowBlur: "Schattenunschärfe (0 bis 50)",
  shadowBlurShort: "Schattenunschärfe",
  addSpeaker: "Sprecher hinzufügen",
  removeSpeaker: "Sprecher entfernen",
  resetPalette: "Ursprüngliche Farben wiederherstellen",
  modelsHeading: "Modelle",
  noRecommendedSpeakerModel:
    "Für diesen Computer wird kein Nemotron-3.5-Modell empfohlen. Untertitel für 1 Sprecher kannst du trotzdem machen.",
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
    no_model_for_role: "Für {role} ist kein Modell installiert. Lade eines unter Modelle.",
    needs_nvidia: "{model} benötigt eine NVIDIA GPU. Wähle unter „Modelle“ ein anderes Modell.",
    needs_memory:
      "{model} benötigt {required} GB Arbeitsspeicher; dieser Computer hat {available} GB. Wähle ein kleineres Modell.",
    too_long_for_model:
      "{model} verarbeitet Aufnahmen bis {limit} Minuten. Dieses Video ist länger. Stelle die Anzahl der Sprecher auf 1.",
    transcript_missing:
      "Neben {file} liegt keine Transkription, daher kann die Datei nicht neu geschrieben werden. Lege die gleichnamige .transcript.json-Datei in denselben Ordner.",
    profile_exists:
      "Ein Profil namens {name} gibt es bereits. Wähle einen anderen Namen oder überschreibe es in der Liste.",
    profile_missing: "Das Profil {name} gibt es nicht mehr.",
    models_busy:
      "Während eine Aufgabe oder ein Download läuft, können Modelle nicht zurückgesetzt werden. Warte das Ende ab oder brich ab und versuche es erneut.",
    models_missing:
      "Die Videos wurden nicht hinzugefügt, weil für Folgendes kein Modell verfügbar ist: {roles}. Lade sie unter Modelle herunter oder füge einen Ordner hinzu, der sie enthält.",
    update_busy:
      "Während Videos bearbeitet oder Modelle geladen werden, geht kein Update. Warte, bis sie fertig sind, und versuche es noch einmal.",
    no_supported_files:
      "Diese Datei geht nicht. Wähle ein Video (.mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts) oder eine Untertiteldatei (.drt .fcpxml .srt .ttml .transcript.json).",
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
              "Wähle bei „{subtitleFormat}“ die Dateiart. Am Anfang steht dort DRT. Darunter steht, wofür sie gut ist.",
              "Ziehe das Video in das Feld und lass es los. Ziehen heißt: gedrückt halten und bewegen. Du kannst auch „{pickVideos}“ drücken.",
            ],
            notes: [
              "Das „?“ neben der Anzahl erklärt, warum sie hilft.",
              "Spricht nur eine Person, lass 1 stehen. Das geht schneller.",
              "Andere Anzahlen kannst du wählen, sobald ein Nemotron-3.5-Modell auf diesem Computer ist. Du holst es unter „{tabModels}“.",
              "Diese Videos gehen: .mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts. Untertiteldateien: .drt .fcpxml .srt .ttml .transcript.json. Bei anderen Dateien sagt die App, dass sie nicht gehen.",
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
              ".drt: Text+-Untertitel für DaVinci Resolve mit Farben, Konturen, äußeren Rändern und Schatten.",
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
            heading: "In DaVinci Resolve holen",
            intro:
              "Eine .drt-Datei holt die Untertitel als Text+-Clips, mit Farben, Konturen, äußeren Rändern und Schatten.",
            steps: [
              "Öffne ein Projekt in DaVinci Resolve.",
              "Wähle im Menü File > Import > Timeline...",
              "Wähle die .drt-Datei, die die App gemacht hat.",
              "Drücke in der Rückfrage Import.",
              "Die Untertitel kommen als Text+-Clips. Wähle einen Clip aus. Im Inspector änderst du unter Text die Wörter und unter Shading die Farben und Ränder.",
            ],
            notes: [
              "Die Nummern von Select Element: 1 ist die Füllung, 2 die Kontur, 4 der äußere Rand, 3 der Schatten, 5 die Stärke der Füllung.",
              "Bei einem neuen Projekt kann „Automatically set project setting“ an bleiben. Fügst du sie zu einem Projekt hinzu, an dem du arbeitest, schalte es aus, damit die Auflösung gleich bleibt.",
              "Verschiebe die Videodatei nicht.",
            ],
          },
          {
            heading: "Eine FCPXML-Datei holen",
            intro:
              "Eine FCPXML-Datei holt Video und Untertitel auf einmal, mit Farben, Rändern und Positionen.",
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
              "Stelle mit dem Schieberegler die Textgröße ein. Ein Schieberegler ist ein Griff, den du nach links und rechts bewegst. Du kannst auch eine Zahl eingeben.",
              "Wähle einen Sprecher und stelle Füllung, Kontur, äußeren Rand und Schatten ein. Drücke ein Farbquadrat, um eine Farbe zu wählen. „Transparent“ entfernt den äußeren Rand oder den Schatten.",
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
              "Ziehe eine .drt-, .fcpxml-, .srt-, .ttml- oder .transcript.json-Datei in das Feld und lass sie los. Du kannst auch „{pickSubtitles}“ drücken.",
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
      settings: {
        heading: "Einstellungen und Updates",
        parts: [
          {
            heading: "Die Einstellungen",
            intro:
              "Der Tab „{tabSettings}“ zeigt die Version der App. Deine Auswahl bleibt für das nächste Mal.",
            steps: [
              "Öffne den Tab „{tabSettings}“.",
              "Wähle bei „{uiLanguageLabel}“ eine Sprache. Der Bildschirm ändert sich sofort.",
              "Wähle bei „{uiThemeLabel}“ „{uiThemeLight}“ oder „{uiThemeDark}“. „{uiThemeSystem}“ folgt deinem Computer.",
            ],
            notes: [],
          },
          {
            heading: "Die App aktualisieren",
            intro:
              "Gibt es eine neue Version, zeigt die App sie oben im Fenster. Bis du den Knopf drückst, ändert sich nichts.",
            steps: [
              "Willst du selbst nachsehen, drücke im Tab „{tabSettings}“ auf „{updateCheck}“.",
              "Lies, was neu ist, und drücke „{updateInstall}“.",
              "Die App lädt das Update und startet neu.",
            ],
            notes: [
              "Während Videos bearbeitet oder Modelle geladen werden, kannst du nicht aktualisieren. Warte, bis sie fertig sind.",
              "Für die Suche und das Laden brauchst du eine Internetverbindung.",
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
            heading: "Du kannst die Anzahl der Sprecher nicht wählen",
            intro:
              "Dafür brauchst du ein Nemotron-3.5-Modell. Ohne es macht die App Untertitel für 1 Sprecher.",
            steps: [],
            notes: [
              "Passt ein Nemotron-3.5-Modell zu diesem Computer, lade es im Tab „{tabModels}“.",
              "Passt keines, kannst du trotzdem Untertitel für 1 Sprecher machen.",
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
              "In einer .drt/FCPXML-Datei stehen Ort und Name des Videos. Verschiebst du das Video oder benennst es um, findet das Schnittprogramm es nicht.",
            steps: [
              "Lege das Video zurück an seinen alten Ort, mit seinem alten Namen.",
              "Öffne die .drt/.fcpxml-Datei noch einmal.",
            ],
            notes: [],
          },
        ],
      },
    },
    imageFolder: "de",
    resolveFigures: {
      menu: "Wähle File, dann Import, dann Timeline. 1 ist File, 2 Import, 3 Timeline.",
      dialog: "Die Rückfrage Import Timeline. 1 ist Automatically set project setting, 2 der Knopf Import.",
      timeline: "Die Timeline nach dem Import. 1 sind die Text+-Clips auf V2.",
      text: "Der Inspector mit einem gewählten Clip. 1 ist der Tab Text, 2 das Feld für die Wörter.",
      shading: "Der Tab Shading im Inspector. 1 ist Shading, 2 sind die Nummern von Select Element.",
    },
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
