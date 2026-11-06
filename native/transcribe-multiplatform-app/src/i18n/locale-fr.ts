// Runs with bun: French UI text.
import type { Messages } from "./messages.ts";

export const frMessages: Messages = {
  appName: "Transcribe Captions",
  tabJobs: "Transcrire",
  tabStyle: "Apparence et position des sous-titres",
  tabSettings: "Réglages",
  uiLanguageLabel: "Langue de l'interface",
  uiLanguageSystem: "Comme le système",
  uiThemeLabel: "Apparence",
  uiThemeSystem: "Comme le système",
  uiThemeLight: "Clair",
  uiThemeDark: "Sombre",
  updateCheck: "Rechercher une mise à jour",
  updateChecking: "Recherche d'une mise à jour",
  updateUpToDate: "C'est la dernière version.",
  updateCheckFailed: "Impossible de rechercher une mise à jour : {detail}",
  updateAvailable: "La version {version} est disponible",
  updateInstall: "Mettre à jour et redémarrer",
  updateRetry: "Réessayer la mise à jour",
  updateBusy:
    "Tu pourras mettre à jour quand les vidéos et les téléchargements de modèles en cours seront terminés.",
  updateInstalling: "Téléchargement de la mise à jour. L'application redémarre ensuite.",
  updateProgressLabel: "Téléchargement de la mise à jour",
  updateInstallFailed: "La mise à jour n'a pas abouti : {detail}",
  tabHelp: "Mode d'emploi",
  tabModels: "Modèles",
  noticeSpeakersOnCpu:
    "La distinction des voix tourne sur le CPU, donc c'est plus lent. Télécharge les données GeForce dans Modèles pour aller plus vite.",
  noticeCpuOnly:
    "Aucun GPU utilisable par cette version n'a été trouvé. Tout tourne sur le CPU, donc c'est plus lent.",
  dropTitle: "Dépose ici des vidéos ou des fichiers de sous-titres",
  dropHint:
    "Les vidéos donnent des fichiers de sous-titres pour logiciel de montage, enregistrés à côté de chaque vidéo.",
  dropHover: "Relâche pour ajouter les fichiers",
  pickVideos: "Choisir des vidéos",
  pickSubtitles: "Choisir des fichiers de sous-titres",
  subtitleFilterName: "Fichiers de sous-titres",
  dropProfileHint:
    "Les fichiers de sous-titres déposés sont réécrits dans de nouveaux fichiers avec le profil enregistré {name}.",
  dropCurrentHint:
    "Les fichiers de sous-titres déposés sont réécrits dans de nouveaux fichiers avec les réglages actuels des sous-titres.",
  speakerCountHelpLabel: "Pourquoi indiquer le nombre de locuteurs ?",
  speakerCountHelp:
    "Les couleurs des locuteurs dépendent de la distinction des voix. Indiquer le nombre évite que des voix proches se confondent ou qu'une voix soit scindée. Avec 1 locuteur, la séparation est ignorée : c'est plus rapide et aucun modèle de locuteurs n'est nécessaire.",
  speakerLoadLight: "Léger",
  speakerLoadHeavy: "Charge élevée",
  speakerOptionLoad: "{label} ({load})",
  speakerLoadLightDetail: "Utilise Whisper seulement.",
  speakerLoadHeavyDetail:
    "Distingue les voix avec Nemotron. Cela prend beaucoup plus de temps et de mémoire.",
  speakerCountOnlyOne:
    "Sans modèle Nemotron 3.5, seul 1 locuteur est possible. Télécharge-en un dans « {tabModels} » pour en choisir plus.",
  profilesHeading: "Profils",
  profilesHint:
    "Enregistre l'apparence et la position des sous-titres sous un nom, puis restaure-les plus tard. Les modèles et la langue parlée ne font pas partie d'un profil.",
  profileName: "Nom du profil",
  profileSave: "Enregistrer comme nouveau profil",
  profileEmpty: "Aucun profil enregistré pour l'instant.",
  profileSelected: "Sélectionné",
  profileChanged: "Modifications non enregistrées",
  profileLoad: "Charger {name}",
  profileOverwrite: "Écraser {name}",
  profileOverwriteTitle: "Écraser {name} ?",
  profileOverwriteDescription:
    "L'apparence enregistrée est remplacée par les réglages actuels. Cette action est irréversible.",
  profileDelete: "Supprimer {name}",
  profileDeleteTitle: "Supprimer {name} ?",
  profileDeleteDescription:
    "Le profil est supprimé. Les réglages actuels et les fichiers de sous-titres existants ne changent pas. Cette action est irréversible.",
  profileExport: "Exporter {name}",
  profileImport: "Importer depuis un fichier",
  profileFileFilter: "Profils",
  resetLook: "Rétablir l'aspect et la position d'origine",
  resetLookTitle: "Rétablir l'aspect et la position d'origine ?",
  resetLookCurrent:
    "L'aspect et la position actuels reviennent aux réglages d'origine de l'application. Les autres profils, le nombre de locuteurs, les modèles et la langue ne changent pas. Cette action est irréversible.",
  resetLookProfile:
    "L'aspect et la position du profil {name} reviennent aux réglages d'origine de l'application. Les autres profils, le nombre de locuteurs, les modèles et la langue ne changent pas. Cette action est irréversible.",
  resetLookConfirm: "Rétablir",
  dialogCancel: "Annuler",
  fontFamily: "Famille de police",
  fontSearch: "Rechercher une police",
  fontHint: "Utilisée pour tous les locuteurs. Saisis du texte pour filtrer la liste.",
  fontLoading: "Lecture des polices installées",
  fontNoMatch: "Aucune police installée ne correspond",
  fontNotInstalled:
    "{font} n'est pas installée sur cet ordinateur. L'aperçu utilise une police de remplacement et les fichiers de sous-titres gardent ce nom.",
  downloadAllLabel: "Télécharger ce qui manque",
  downloadAll: "{label} : {count} ({size})",
  resetModels: "Réinitialiser les modèles et les retélécharger",
  resetModelsTitle: "Réinitialiser les modèles et les retélécharger ?",
  resetModelsDescription:
    "Tout ce que cette application a téléchargé dans {path} est supprimé, puis les modèles nécessaires à cet ordinateur sont retéléchargés. Les dossiers que tu as ajoutés ne sont jamais modifiés. Cette action est irréversible.",
  resetModelsConfirm: "Supprimer et retélécharger",
  videoFilterName: "Vidéos",
  readinessMissing:
    "Aucun modèle n’est disponible pour : {roles}. Télécharges-en un dans Modèles, ou ajoute un dossier qui en contient déjà un.",
  roleTranscribe: "la transcription",
  roleDiarize: "la séparation des voix",
  roleFace: "la détection des visages",
  roleRuntime: "l’environnement d’exécution",
  jobQueued: "En attente",
  jobCancelled: "Annulé",
  jobCancel: "Annuler {name}",
  jobRelayout: "Réécrire {name} avec la position et les couleurs actuelles",
  jobDone: "Terminé : {cues} sous-titres, {speakers} intervenants",
  jobDoneOneSpeaker: "Terminé : {cues} sous-titres, 1 intervenant",
  jobFaceless: "Les sous-titres sans visage détecté ont été placés en bas de la vidéo.",
  jobSpeakersMissing:
    "Le modèle de locuteurs n’a détecté personne dans cette vidéo : tous les sous-titres ont donc la même couleur.",
  jobFontMissing:
    "La police choisie n’est pas sur cet ordinateur : la largeur des sous-titres est donc estimée.",
  jobNoPercent: "ce modèle n’indique pas de pourcentage",
  speakerCount: "Nombre de locuteurs",
  subtitleFormat: "Format du fichier de sous-titres",
  speakerCountAuto: "Détection automatique",
  speakerCountValue: "{count} locuteurs",
  speakerCountOne: "1 locuteur",
  jobFewerSpeakers:
    "{requested} locuteurs demandés, mais seulement {found} trouvés. Cela arrive avec des enregistrements courts ou des voix proches.",
  jobStep: "Étape {number} sur {total} : {stage}",
  jobEstimating: "Estimation du temps restant",
  jobRemaining: "Environ {minutes} min restantes",
  jobRemainingUnderMinute: "Moins d'une minute restante",
  stepStates: { done: "terminé", current: "en cours", pending: "en attente" },
  outputPurposes: {
    drt: "Sous-titres Text+ pour DaVinci Resolve, avec couleurs, contours, contours extérieurs et ombres",
    ttml: "Sous-titres stylés (couleur, contour, position)",
    srt: "Sous-titres avec les mots et les temps (couleurs parfois absentes, ni contours ni positions)",
    fcpxml: "Timeline avec titres",
    json: "Données de transcription pour réécrire",
  },
  jobProgressLabel: "Progression de {name}",
  jobOutputs: "Fichiers écrits",
  stages: {
    convert_model: "Conversion du modèle",
    extract_audio: "Extraction de l’audio",
    transcribe: "Transcription",
    diarize: "Séparation des voix",
    faces: "Détection des visages",
    export: "Écriture des fichiers de sous-titres",
  },
  positionHeading: "Position des sous-titres",
  positionLabel: "Placer les sous-titres",
  anchorFaceAbove: "Au-dessus du visage",
  anchorFaceBelow: "Sous le visage",
  anchorVideoTop: "En haut de la vidéo",
  anchorVideoBottom: "En bas de la vidéo",
  offsetX: "Ajustement gauche-droite (pourcentage de la largeur)",
  offsetY: "Ajustement haut-bas (pourcentage de la hauteur)",
  safeZoneHint:
    "Définit la largeur des bords gauche et droit où aucun sous-titre n'est placé. L'aperçu les montre en bandes sombres.",
  safeLeft: "Zone de sécurité gauche (pourcentage de la largeur)",
  safeRight: "Zone de sécurité droite (pourcentage de la largeur)",
  faceModel: "Modèle de détection des visages",
  speechLanguage: "Langue parlée",
  languageAuto: "Détecter automatiquement",
  sizeHeading: "Taille des sous-titres",
  sizeHint:
    "Les tailles suivent automatiquement la résolution de chaque vidéo. Ce sont des proportions de la vidéo.",
  fontSize: "Taille du texte (pourcentage du petit côté)",
  captionWidth: "Ligne la plus longue (pourcentage de la largeur)",
  captionLineHeight: "Interligne (multiple de la taille du texte)",
  contrastWarning:
    "{name} : le contraste entre le remplissage et le contour est de {ratio}:1, sous le minimum WCAG AA de 4,5:1. Choisis des couleurs plus éloignées.",
  previewHeading: "Aperçu",
  previewFrameLabel: "Taille de l’aperçu",
  previewDescription:
    "Sous-titre de l’aperçu à {x} % en partant de la gauche et à {y} % en partant du haut du cadre {size}.",
  previewFontPixels: "Taille du texte à cette résolution : {px} px.",
  previewSample: "Les sous-titres contourés changent de couleur selon l’intervenant",
  previewLandscape: "1920×1080",
  previewVertical: "Vertical 1080×1920",
  previewUhd: "4K",
  speakersHeading: "Couleurs des intervenants (jusqu’à {count} intervenants)",
  speakersHint:
    "Choisis un locuteur pour changer ses couleurs et ses épaisseurs. L'aperçu montre ses sous-titres. L'épaisseur et la position sont des pourcentages de la taille du texte.",
  speakerName: "Intervenant {number}",
  colorColumn: "Couleur",
  fillWidth: "Épaisseur du remplissage (pourcentage de la taille du texte)",
  outlineWidth: "Épaisseur du contour (pourcentage de la taille du texte)",
  rimWidth: "Épaisseur du contour extérieur (pourcentage de la taille du texte)",
  layerWidth: "Épaisseur (pourcentage de la taille du texte)",
  colorTransparent: "Transparent",
  colorCustom: "Couleur libre",
  colorNames: {
    white: "Blanc",
    black: "Noir",
    red: "Rouge",
    orange: "Orange",
    yellow: "Jaune",
    green: "Vert",
    blue: "Bleu",
    purple: "Violet",
  },
  fillLabel: "Remplissage",
  outlineLabel: "Contour",
  rimLabel: "Contour extérieur",
  formatLimits: {
    fcpxml: "FCPXML n'affiche pas l'ombre. Choisis DRT pour l'utiliser.",
    ttml: "TTML n'affiche ni contour extérieur ni ombre. Choisis DRT pour les utiliser.",
    srt: "SRT ne garde que la couleur du texte : ni contour, ni contour extérieur, ni ombre. Choisis DRT pour les utiliser.",
  },
  shadowLabel: "Ombre",
  shadowColor: "Couleur de l'ombre",
  shadowX: "Ombre gauche-droite (pourcentage de la taille du texte)",
  shadowXShort: "Ombre gauche-droite",
  shadowY: "Ombre haut-bas (pourcentage de la taille du texte ; négatif vers le bas)",
  shadowYShort: "Ombre haut-bas",
  shadowBlur: "Flou de l'ombre (0 à 50)",
  shadowBlurShort: "Flou de l'ombre",
  addSpeaker: "Ajouter un intervenant",
  removeSpeaker: "Retirer un intervenant",
  resetPalette: "Rétablir les couleurs d’origine",
  modelsHeading: "Modèles",
  noRecommendedSpeakerModel:
    "Aucun modèle Nemotron 3.5 n'est recommandé pour cet ordinateur. Tu peux quand même créer des sous-titres pour 1 locuteur.",
  modelsHint:
    "Les modèles adaptés à cet ordinateur sont marqués comme recommandés. Les modèles déjà téléchargés sont retrouvés et réutilisés.",
  addModelFolder: "Ajouter un dossier contenant des modèles existants",
  modelFolders: "Dossiers explorés",
  badgeRecommended: "Recommandé ici",
  badgePresent: "Disponible",
  useModel: "Utiliser {name}",
  inUseModel: "Utilisé : {name}",
  download: "Télécharger {name} ({size})",
  downloadCancel: "Annuler le téléchargement de {name}",
  downloadProgress: "{done} sur {total}",
  downloadProgressLabel: "Téléchargement de {name}",
  deleteModel: "Supprimer la copie téléchargée de {name}",
  sourceLabel: "Source : {url}",
  locationLabel: "Emplacement : {path}",
  loadFailed: "Un problème est survenu : {detail}",
  errors: {
    no_video_stream: "Ce fichier ne contient pas de vidéo. Choisis un fichier vidéo.",
    no_audio_stream: "Cette vidéo n’a pas de son : il n’y a rien à transcrire.",
    unreadable_size: "La taille de cette vidéo est illisible. Le fichier est peut-être endommagé.",
    unreadable_duration: "La durée de cette vidéo est illisible. Le fichier est peut-être endommagé.",
    no_words: "Aucun mot n’a été trouvé dans l’audio.",
    model_missing: "{model} n’est pas installé. Télécharge-le dans Modèles.",
    no_model_for_role: "Aucun modèle n’est installé pour {role}. Télécharge-en un dans Modèles.",
    needs_nvidia: "{model} nécessite un GPU NVIDIA. Choisis un autre modèle dans Modèles.",
    needs_memory:
      "{model} nécessite {required} Go de mémoire ; cet ordinateur en a {available} Go. Choisis un modèle plus petit.",
    too_long_for_model:
      "{model} accepte les enregistrements jusqu'à {limit} minutes. Cette vidéo est plus longue. Règle le nombre de locuteurs sur 1.",
    transcript_missing:
      "{file} n'a pas de transcription à côté de lui ; il ne peut donc pas être réécrit. Garde le fichier .transcript.json du même nom dans le même dossier.",
    profile_exists: "Un profil nommé {name} existe déjà. Choisis un autre nom ou écrase-le depuis la liste.",
    profile_missing: "Le profil {name} n'existe plus.",
    models_busy:
      "Les modèles ne peuvent pas être réinitialisés pendant une tâche ou un téléchargement. Attends la fin ou annule, puis réessaie.",
    models_missing:
      "Les vidéos n'ont pas été ajoutées, car aucun modèle n'est disponible pour : {roles}. Télécharge-les dans Modèles ou ajoute un dossier qui les contient.",
    update_busy:
      "La mise à jour ne peut pas démarrer pendant le traitement de vidéos ou le téléchargement de modèles. Attends la fin, puis réessaie.",
    no_supported_files:
      "Ce fichier n'est pas accepté. Choisis une vidéo (.mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts) ou un fichier de sous-titres (.drt .fcpxml .srt .ttml .transcript.json).",
    profile_invalid:
      "{file} ne peut pas être lu comme un profil. Choisis un fichier exporté par cette application.",
    internal: "{detail}",
    cancelled: "Annulé",
  },
  modelNotes: {
    "whisper-large-v3-turbo": "Transcription. Le même modèle fonctionne sur Mac (Metal) et Windows (Vulkan).",
    "nemotron-3.5-transcribe-iq4":
      "Séparation des voix. Pour les Mac de 32 Go ou plus, les GPU de 24 Go ou plus et les CPU.",
    "nemotron-3.5-transcribe-nvfp4":
      "Séparation des voix. Pour les GPU Blackwell comme la série GeForce RTX 50.",
    "nemotron-3.5-transcribe-q2m":
      "Séparation des voix. Pour les GPU de 16 Go ; chaque enregistrement peut durer environ 7 minutes.",
    "nemotron-3.5-transcribe-q2l": "Séparation des voix. Pour les Mac de 24 Go.",
    "nemotron-3.5-transcribe-q8":
      "Séparation des voix. Qualité maximale, pour les Mac de 64 Go et les GPU de 48 Go.",
    yunet: "Visages réels. Fourni avec l’app ; donne le même résultat sur Mac et Windows.",
    "avatar-face-yolo":
      "Visages d’avatars. Fonctionne une fois le fichier ONNX exporté placé dans un dossier de modèles.",
    "geforce-runtime":
      "Version CUDA de l’outil de séparation des voix. Sans elle, la séparation s’exécute sur le CPU.",
  },
  help: {
    version: "Version {version}",
    intro:
      "Cette application transforme ce qui est dit dans une vidéo en sous-titres, avec une couleur par personne. Tu peux utiliser ces sous-titres dans un logiciel de montage vidéo.",
    sections: {
      start: {
        heading: "Pour commencer",
        parts: [
          {
            heading: "Installer l'application",
            intro: "Télécharge le fichier qui convient à ton ordinateur, puis installe-le.",
            steps: [
              "Choisis dans la liste ci-dessous le fichier pour ton ordinateur.",
              "Sur Mac, ouvre le zip et mets l'application dans le dossier « Applications ».",
              "Sur Windows, ouvre le exe et suis les étapes.",
            ],
            notes: [
              "Mac : Transcribe-Captions-macos-arm64.zip",
              "Windows avec GeForce : Transcribe-Captions-windows-geforce-x64-setup.exe",
              "Windows avec Radeon : Transcribe-Captions-windows-radeon-x64-setup.exe",
              "Windows sans aucune des deux : n'importe quel exe convient. Il tourne seulement sur le CPU, donc c'est plus lent.",
              "Si le Mac te bloque : ouvre Réglages Système > Confidentialité et sécurité et appuie sur « Ouvrir quand même ».",
              "Si Windows te bloque : appuie sur « Informations complémentaires », puis sur « Exécuter quand même ».",
            ],
          },
          {
            heading: "Télécharger les modèles",
            intro:
              "Les modèles sont les données qui servent à changer la parole en texte. Il suffit de les télécharger une fois.",
            steps: [
              "Ouvre l'onglet « {tabModels} ».",
              "Appuie sur « {downloadAllLabel} ».",
              "Attends que la barre de chaque modèle arrive au bout.",
            ],
            notes: [
              "Un modèle prêt affiche « {badgePresent} ».",
              "S'il ne manque rien, ce bouton n'apparaît pas.",
              "Il faut Internet. Les fichiers sont gros, cela prend du temps.",
              "Si un téléchargement s'arrête, appuie de nouveau sur le bouton. Il reprend là où il s'est arrêté.",
            ],
          },
          {
            heading: "Utiliser des modèles que tu as déjà",
            intro: "Si tu as déjà téléchargé des modèles, inutile de recommencer.",
            steps: [
              "Ouvre l'onglet « {tabModels} ».",
              "Appuie sur « {addModelFolder} ».",
              "Choisis le dossier qui contient les modèles.",
            ],
            notes: ["Les modèles trouvés affichent « {badgePresent} »."],
          },
        ],
      },
      transcribe: {
        heading: "Créer des sous-titres à partir d'une vidéo",
        parts: [
          {
            heading: "Ajouter une vidéo",
            intro: "Quand tu ajoutes une vidéo, l'application commence les sous-titres.",
            steps: [
              "Ouvre l'onglet « {tabJobs} ».",
              "« {speakerCount} » est réglé sur 1 au départ. Si plus de personnes parlent, choisis leur nombre. Si tu ne sais pas, choisis « {speakerCountAuto} ».",
              "Dans « {subtitleFormat} », choisis le type de fichier. Au départ, c'est DRT. La ligne en dessous dit à quoi il sert.",
              "Fais glisser la vidéo dans le cadre et lâche-la. Faire glisser, c'est garder appuyé en bougeant. Tu peux aussi appuyer sur « {pickVideos} ».",
            ],
            notes: [
              "Le « ? » à côté du nombre explique à quoi il sert.",
              "Si une seule personne parle, laisse 1. C'est plus rapide.",
              "1 locuteur est léger : seul Whisper tourne. La détection automatique et 2 locuteurs ou plus demandent une charge élevée : Nemotron tourne aussi, ce qui prend beaucoup plus de temps et de mémoire. Tu peux les choisir dès qu'un modèle Nemotron 3.5 est sur cet ordinateur. Tu le télécharges dans « {tabModels} ».",
              "Vidéos acceptées : .mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts. Fichiers de sous-titres : .drt .fcpxml .srt .ttml .transcript.json. Avec un autre fichier, l'application dit qu'il n'est pas accepté.",
              "Si tu ajoutes plusieurs vidéos, elles sont traitées une par une.",
              "La première fois, l'application met aussi un modèle sous une forme qu'elle peut utiliser.",
              "Si la langue des sous-titres n'est pas la bonne, choisis-la dans « {speechLanguage} », dans l'onglet « {tabStyle} ».",
            ],
          },
          {
            heading: "Suivre l'avancement",
            intro: "Une fois la vidéo ajoutée, une liste apparaît sous le cadre.",
            steps: [],
            notes: [
              "Elle montre l'étape en cours et le nombre d'étapes.",
              "La barre montre l'avancement de tout le travail.",
              "Elle indique aussi à peu près le temps restant.",
              "Pour arrêter, appuie sur le bouton à côté du nom de la vidéo.",
            ],
          },
          {
            heading: "Les fichiers créés",
            intro:
              "Deux fichiers apparaissent dans le dossier de la vidéo : le fichier de sous-titres du type choisi et un fichier .transcript.json. Les anciens sont gardés. Si le nom est déjà pris, les nouveaux reçoivent -2 dans leur nom.",
            steps: [],
            notes: [
              ".drt : sous-titres Text+ pour DaVinci Resolve, avec couleurs, contours, contours extérieurs et ombres.",
              ".fcpxml : les sous-titres avec la vidéo, pour logiciel de montage.",
              ".ttml : des sous-titres avec couleurs et positions.",
              ".srt : des sous-titres avec les mots et les temps. Les couleurs peuvent ne pas s'afficher. Il n'y a ni contours ni positions.",
              ".transcript.json : les données pour réécrire les sous-titres. Garde-le.",
            ],
          },
        ],
      },
      editor: {
        heading: "Ouvrir dans le logiciel de montage",
        parts: [
          {
            heading: "Importer dans DaVinci Resolve",
            intro:
              "Un fichier .drt importe les sous-titres en clips Text+, avec leurs couleurs, contours, contours extérieurs et ombres.",
            steps: [
              "Ouvre un projet dans DaVinci Resolve.",
              "Dans le menu, choisis File > Import > Timeline...",
              "Choisis le fichier .drt créé par l'application.",
              "À l'écran de confirmation, appuie sur Import.",
              "Les sous-titres arrivent en clips Text+. Sélectionne un clip : dans l'Inspector, Text change les mots et Shading les couleurs et les bords.",
            ],
            notes: [
              "Les numéros de Select Element : 1 est le remplissage, 2 le contour, 4 le contour extérieur, 3 l'ombre, 5 l'épaisseur du remplissage.",
              "Pour un nouveau projet, « Automatically set project setting » peut rester activé. Si tu ajoutes à un projet en cours, désactive-le pour que la résolution ne change pas.",
              "Ne déplace pas le fichier vidéo.",
            ],
          },
          {
            heading: "Importer un fichier FCPXML",
            intro:
              "Un fichier FCPXML apporte la vidéo et les sous-titres d'un coup, avec leurs couleurs, contours et positions.",
            steps: [
              "Ouvre un projet dans ton logiciel de montage. Un projet contient tout ton travail de montage.",
              "Dans le menu, importe la timeline (par exemple File > Import > Timeline).",
              "Choisis le fichier .fcpxml créé par l'application.",
              "La vidéo et les sous-titres arrivent sur une timeline. Une timeline, c'est l'endroit où la vidéo est rangée dans l'ordre du temps.",
            ],
            notes: [
              "Chaque sous-titre a 2 ou 3 couches l'une sur l'autre. Pour changer les mots, change toutes les couches.",
            ],
          },
          {
            heading: "Quand utiliser les autres types",
            intro: "Pour un autre type, choisis-le dans « {subtitleFormat} » avant d'ajouter la vidéo.",
            steps: [],
            notes: [
              "Pour un fichier .ttml, choisis Import Subtitle dans le Media Pool. Le Media Pool, c'est l'endroit où sont rangés tes vidéos et sous-titres.",
              "Utilise le fichier .srt pour corriger les mots ou les temps dans ton logiciel de montage. Si cette application réécrit les sous-titres, tes corrections n'y sont pas.",
              "N'utilise pas FCPXML et TTML sur la même timeline. Les sous-titres s'afficheraient deux fois.",
            ],
          },
        ],
      },
      look: {
        heading: "Changer l'aspect et la place des sous-titres",
        parts: [
          {
            heading: "Changer l'aspect et la place",
            intro:
              "Tes changements se voient tout de suite dans l'aperçu et s'enregistrent tout seuls. L'aperçu montre à quoi ressembleront les sous-titres.",
            steps: [
              "Ouvre l'onglet « {tabStyle} ».",
              "Dans « {positionHeading} », choisis « {anchorFaceAbove} », « {anchorFaceBelow} », « {anchorVideoTop} » ou « {anchorVideoBottom} ».",
              "Règle la taille du texte avec le curseur. Un curseur est une poignée à bouger à gauche et à droite. Tu peux aussi taper un nombre.",
              "Choisis un locuteur, puis règle le remplissage, le contour, le contour extérieur et l'ombre. Appuie sur un carré de couleur pour choisir une couleur. « Transparent » retire le contour extérieur ou l'ombre.",
            ],
            notes: [
              "L'aperçu est à droite sur un grand écran et en haut sur un petit. Il reste visible quand tu fais défiler.",
              "Pour éloigner les sous-titres des bords gauche et droit, règle les zones de sécurité dans « {positionHeading} ». Les bandes sombres de l'aperçu les montrent.",
            ],
          },
          {
            heading: "Choisir une police",
            intro:
              "La police, c'est la forme des lettres. Tu choisis parmi celles de cet ordinateur. Tout le monde a la même.",
            steps: [
              "Sous « {fontFamily} », appuie à l'endroit où s'affiche le nom de la police.",
              "Tape une partie du nom. La liste devient plus courte.",
              "Appuie sur la police que tu veux.",
            ],
            notes: [],
          },
          {
            heading: "Enregistrer et charger un profil",
            intro: "Un profil, ce sont des réglages d'aspect et de place enregistrés sous un nom.",
            steps: [
              "Tape un nom.",
              "Appuie sur « {profileSave} ».",
              "Pour t'en servir plus tard, appuie sur son bouton de chargement dans la liste.",
            ],
            notes: [
              "L'application demande avant d'écraser ou de supprimer un profil.",
              "Les modèles et la langue parlée ne font pas partie du profil.",
            ],
          },
          {
            heading: "Utiliser un profil sur un autre ordinateur",
            intro: "Exporte un profil dans un fichier pour l'utiliser sur un autre ordinateur.",
            steps: [
              "Dans la liste, appuie sur le bouton d'export et choisis où enregistrer.",
              "Copie le fichier sur l'autre ordinateur.",
              "Sur l'autre ordinateur, appuie sur « {profileImport} » et choisis le fichier.",
            ],
            notes: [
              "Un profil importé apparaît dans la liste.",
              "Si le nom est déjà pris, il arrive sous un autre nom, sans rien remplacer.",
            ],
          },
          {
            heading: "Revenir à l'aspect d'origine",
            intro: "Tu peux remettre l'aspect et la position aux réglages d'origine de l'application.",
            steps: [
              "Ouvre l'onglet « {tabStyle} ».",
              "Appuie sur « {resetLook} ».",
              "Lis le message, puis continue.",
            ],
            notes: [
              "Si tu utilises un profil, seul ce profil revient en arrière.",
              "Le nombre de locuteurs, les modèles, la langue et les autres profils ne changent pas.",
            ],
          },
        ],
      },
      rewrite: {
        heading: "Donner un nouvel aspect à des sous-titres finis",
        parts: [
          {
            heading: "Ajouter un fichier de sous-titres",
            intro:
              "Tu peux changer l'aspect sans refaire les sous-titres à partir du son. C'est souvent rapide. C'est plus long si l'application doit chercher des visages.",
            steps: [
              "Charge le profil que tu veux. Sinon, les réglages actuels sont utilisés.",
              "Ouvre l'onglet « {tabJobs} ».",
              "Fais glisser un fichier .drt, .fcpxml, .srt, .ttml ou .transcript.json dans le cadre et lâche-le. Tu peux aussi appuyer sur « {pickSubtitles} ».",
              "La liste sous le cadre montre l'avancement et le résultat.",
            ],
            notes: [
              "Le bouton de réécriture, à côté du nom d'une vidéo dans la liste, crée de nouveaux fichiers avec un autre nom. Les anciens sont gardés.",
              "Les fichiers d'origine sont gardés. Les nouveaux reçoivent -2 ou un autre numéro dans leur nom.",
              "Le type du nouveau fichier dépend de « {subtitleFormat} », pas du fichier ajouté.",
              "Le fichier .transcript.json du même nom doit être dans le même dossier.",
              "Quand tu utilises un profil, les changements non enregistrés dans ce profil ne sont pas utilisés.",
            ],
          },
        ],
      },
      settings: {
        heading: "Réglages et mises à jour",
        parts: [
          {
            heading: "Les réglages",
            intro:
              "L'onglet « {tabSettings} » montre la version de l'application. Tes choix restent pour la prochaine fois.",
            steps: [
              "Ouvre l'onglet « {tabSettings} ».",
              "Dans « {uiLanguageLabel} », choisis une langue. L'écran change tout de suite.",
              "Dans « {uiThemeLabel} », choisis « {uiThemeLight} » ou « {uiThemeDark} ». « {uiThemeSystem} » suit ton ordinateur.",
            ],
            notes: [],
          },
          {
            heading: "Mettre à jour l'application",
            intro:
              "Quand une nouvelle version sort, l'application l'indique en haut de la fenêtre. Rien ne change tant que tu n'appuies pas sur le bouton.",
            steps: [
              "Pour chercher toi-même, appuie sur « {updateCheck} » dans l'onglet « {tabSettings} ».",
              "Lis les nouveautés, puis appuie sur « {updateInstall} ».",
              "L'application télécharge la mise à jour et redémarre.",
            ],
            notes: [
              "Tu ne peux pas mettre à jour pendant le traitement de vidéos ou le téléchargement de modèles. Attends la fin.",
              "La recherche et le téléchargement demandent une connexion à Internet.",
            ],
          },
        ],
      },
      trouble: {
        heading: "Si quelque chose ne marche pas",
        parts: [
          {
            heading: "Il manque un modèle",
            intro: "Sans les modèles nécessaires, l'application ne peut pas commencer les sous-titres.",
            steps: [
              "Ouvre l'onglet « {tabModels} ».",
              "Appuie sur « {downloadAllLabel} ».",
              "Ensuite, ajoute de nouveau la vidéo.",
            ],
            notes: [],
          },
          {
            heading: "Tu ne peux pas choisir le nombre de locuteurs",
            intro:
              "Il faut pour cela un modèle Nemotron 3.5. Sans lui, l'application crée des sous-titres pour 1 locuteur.",
            steps: [],
            notes: [
              "Si un modèle Nemotron 3.5 convient à cet ordinateur, télécharge-le dans l'onglet « {tabModels} ».",
              "Si aucun ne convient, tu peux quand même créer des sous-titres pour 1 locuteur.",
            ],
          },
          {
            heading: "Aucun visage n'a été trouvé",
            intro:
              "Sans visage, le sous-titre va en bas de la vidéo. Si tu choisis une place sans visage, les sous-titres restent toujours au même endroit.",
            steps: [
              "Ouvre l'onglet « {tabStyle} ».",
              "Pour la place des sous-titres, choisis le haut ou le bas de la vidéo.",
              "Appuie sur le bouton de réécriture, à côté du nom de la vidéo.",
            ],
            notes: [
              "Si elle n'est pas dans la liste, ajoute le fichier de sous-titres. Si tu utilises un profil, écrase-le d'abord.",
              "Les visages d'avatars demandent un autre modèle. Mets son fichier dans un dossier de modèles, puis choisis-le dans « {faceModel} ».",
            ],
          },
          {
            heading: "Tu veux recommencer",
            intro: "Si un modèle est peut-être abîmé, tu peux le télécharger de nouveau.",
            steps: [
              "Ouvre l'onglet « {tabModels} ».",
              "Appuie sur « {resetModels} ».",
              "Lis le message, puis continue.",
            ],
            notes: [
              "Seul ce que cette application a téléchargé est supprimé. Les dossiers que tu as ajoutés sont gardés.",
              "C'est impossible pendant la création de sous-titres ou un téléchargement.",
              "Après la suppression, le téléchargement des modèles manquants démarre tout seul.",
            ],
          },
          {
            heading: "Le logiciel de montage ne trouve pas la vidéo",
            intro:
              "Un fichier .drt/FCPXML contient la place et le nom de la vidéo. Si tu déplaces ou renommes la vidéo, le logiciel ne la trouve plus.",
            steps: [
              "Remets la vidéo à son ancienne place, avec son ancien nom.",
              "Ouvre de nouveau le fichier .drt/.fcpxml.",
            ],
            notes: [],
          },
        ],
      },
    },
    imageFolder: "fr",
    resolveFigures: {
      menu: "Choisis File, puis Import, puis Timeline. 1 est File, 2 Import, 3 Timeline.",
      dialog: "L'écran Import Timeline. 1 est Automatically set project setting, 2 le bouton Import.",
      timeline: "La timeline après l'import. 1 montre les clips Text+ sur V2.",
      text: "L'Inspector avec un clip sélectionné. 1 est l'onglet Text, 2 l'endroit où changer les mots.",
      shading: "L'onglet Shading de l'Inspector. 1 est Shading, 2 les numéros de Select Element.",
    },
    images: {
      models: "L'onglet Modèles. 1 est l'onglet, 2 le bouton de téléchargement, 3 la barre de chaque modèle.",
      transcribe:
        "L'onglet des sous-titres. 1 est l'onglet, 2 le nombre de locuteurs, 3 le type de fichier, 4 le cadre pour les vidéos.",
      look: "L'onglet d'aspect et de position. 1 est l'onglet, 2 la place des sous-titres, 3 un curseur, 4 les couleurs d'une personne.",
      font: "La liste des polices ouverte. 1 montre le nom de la police, 2 est l'endroit où taper, 3 la police à choisir.",
      profiles:
        "La zone des profils. 1 est l'endroit où taper un nom, 2 le bouton d'enregistrement, 3 le bouton de chargement.",
      rewrite:
        "Après l'ajout d'un fichier de sous-titres. 2 est l'onglet, 3 le cadre pour les fichiers, 4 la liste des résultats.",
    },
  },
};
