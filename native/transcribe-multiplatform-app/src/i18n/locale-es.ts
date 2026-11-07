// Runs with bun: Spanish UI text.
import type { Messages } from "./messages.ts";

export const esMessages: Messages = {
  appName: "Transcribe Captions",
  tabJobs: "Transcribir",
  tabStyle: "Aspecto y posición de los subtítulos",
  tabSettings: "Ajustes",
  uiLanguageLabel: "Idioma de la interfaz",
  uiLanguageSystem: "Igual que el sistema",
  uiThemeLabel: "Apariencia",
  uiThemeSystem: "Igual que el sistema",
  uiThemeLight: "Claro",
  licensesOpen: "Licencias de código abierto",
  licensesLoading: "Cargando las licencias",
  licensesFailed: "No se pudo leer la lista de licencias.",
  licensesNoText: "Este paquete no incluye el texto de la licencia.",
  licensesUnknown: "No indicada",
  licensesClose: "Cerrar",
  uiThemeDark: "Oscuro",
  updateCheck: "Buscar actualizaciones",
  updateChecking: "Buscando actualizaciones",
  updateUpToDate: "Es la última versión.",
  updateCheckFailed: "No se pudieron buscar actualizaciones: {detail}",
  updateAvailable: "Está disponible la versión {version}",
  updateInstall: "Actualizar y reiniciar",
  updateRetry: "Volver a intentar la actualización",
  updateBusy: "Podrás actualizar cuando terminen los vídeos y las descargas de modelos en curso.",
  updateInstalling: "Descargando la actualización. La aplicación se reinicia al terminar.",
  updateProgressLabel: "Descarga de la actualización",
  updateInstallFailed: "La actualización no terminó: {detail}",
  tabHelp: "Cómo se usa",
  tabModels: "Modelos",
  noticeSpeakersOnCpu:
    "Distinguir a los hablantes se hace en la CPU, así que tarda más. Descarga los datos de GeForce en Modelos para que vaya más rápido.",
  noticeCpuOnly:
    "No se encontró ninguna GPU que pueda usar esta versión. Todo se hace en la CPU, así que tarda más.",
  dropTitle: "Suelta aquí vídeos o archivos de subtítulos",
  dropHint:
    "Los vídeos generan archivos de subtítulos para programas de edición, guardados junto a cada vídeo.",
  dropHover: "Suelta para añadir los archivos",
  pickVideos: "Elegir vídeos",
  pickSubtitles: "Elegir archivos de subtítulos",
  subtitleFilterName: "Archivos de subtítulos",
  dropProfileHint:
    "Los archivos de subtítulos soltados se reescriben como archivos nuevos con el perfil guardado {name}.",
  dropCurrentHint:
    "Los archivos de subtítulos soltados se reescriben como archivos nuevos con los ajustes actuales de subtítulos.",
  speakerCountHelpLabel: "¿Por qué indicar el número de hablantes?",
  speakerCountHelp:
    "Los colores de los hablantes dependen de distinguir las voces. Indicar cuántas personas hablan evita que voces parecidas se unan y que una voz se divida. Con 1 hablante se omite la separación, así que es más rápido y no necesita un modelo de hablantes.",
  speakerLoadLight: "Ligero",
  speakerLoadHeavy: "Carga alta",
  speakerOptionLoad: "{label} ({load})",
  speakerLoadLightDetail: "Usa solo Whisper, sin separación de voces ni separación de hablantes.",
  speakerLoadHeavyDetail:
    "Distingue las voces, así que tarda mucho más y usa mucha más RAM o memoria de GPU.",
  speakerFiguresNote:
    "Whisper tardó unos {whisperCpuMinutes} min en la CPU con un vídeo de cerca de una hora, sin repeticiones. Estas cifras se midieron en un Mac y dependen del vídeo y del ordenador. La RAM y la memoria de GPU en Windows no se han medido.",
  memoryUnified: "memoria unificada (RAM y memoria de GPU compartidas)",
  memoryRam: "memoria RAM",
  memoryGpu: "memoria de GPU",
  speakerPlanRuns: "Se ejecuta en: Whisper {whisper}, hablantes {diarize}.",
  speakerPlanRunsSeparate: " Separación de voces {separate}.",
  speakerPlanDeep:
    "Con 3 o más voces a la vez se separa dos veces, así que tarda más. Aparecen hasta 4 líneas.",
  separationBackend: "Cómo se ejecuta la separación de voces",
  separationBackends: { onnx: "CPU (ONNX Runtime)", coreml: "GPU del Mac (Core ML)" },
  speakerPlanLine: "Se ejecuta con: {models}.",
  speakerFactLabels: {
    runs: "Qué se ejecuta",
    rows: "Líneas",
    memory: "RAM y memoria de GPU",
    time: "Tiempo (sin contar Whisper)",
  },
  speakerFacts: {
    runs: "Codificador Nemotron 3.5 y MossFormer2",
    rows: "Las voces a la vez aparecen en varias líneas.",
    memory:
      "En un Mac, {memoryUnified}: {encoderGb} GB para el codificador Nemotron 3.5, unos {separateCoreMlGb} GB (estimación) para MossFormer2 con Core ML, {separateGb} GB en la CPU con ONNX Runtime (tramos de {separateSeconds} segundos). Unos {jobPeakGb} GB en el pico del trabajo.",
    time: "Codificador Nemotron 3.5: por hora de audio, unos {encoderGpuSeconds} segundos en la GPU de un Mac, unos {encoderCpuSeconds} segundos en la CPU. Separación de voces, medida en un vídeo de cerca de una hora: unos {separateCoreMlMinutes} min con Core ML, unos {separateCpuMinutes} min en la CPU. Depende de cuánto del vídeo haya que separar. Con 3 o más voces a la vez se separa dos veces, lo que tarda más.",
  },
  openModelsTab: "Abrir la pestaña «{tabModels}»",
  runWithOneSpeaker: "Ejecutar con 1 hablante",
  roleSeparate: "separación de voces",
  badgeUsedBySteps: "Para 2 o más hablantes",
  speakerRolesHint:
    "Hace falta cuando hablan 2 o más personas, o cuando el número se detecta automáticamente.",
  profilesHeading: "Perfiles",
  profilesHint:
    "Guarda el aspecto y la posición de los subtítulos con un nombre y restáuralos más tarde. Los modelos y el idioma hablado no forman parte de un perfil.",
  profileName: "Nombre del perfil",
  profileSave: "Guardar como perfil nuevo",
  profileEmpty: "Aún no hay perfiles guardados.",
  profileSelected: "Seleccionado",
  profileChanged: "Cambios sin guardar",
  profileLoad: "Cargar {name}",
  profileOverwrite: "Sobrescribir {name}",
  profileOverwriteTitle: "¿Sobrescribir {name}?",
  profileOverwriteDescription:
    "El aspecto guardado se sustituye por los ajustes actuales. No se puede deshacer.",
  profileDelete: "Eliminar {name}",
  profileDeleteTitle: "¿Eliminar {name}?",
  profileDeleteDescription:
    "Se elimina el perfil. Los ajustes actuales y los archivos de subtítulos existentes no cambian. No se puede deshacer.",
  profileExport: "Exportar {name}",
  profileImport: "Importar desde un archivo",
  profileFileFilter: "Perfiles",
  resetLook: "Volver al aspecto y lugar originales",
  resetLookTitle: "¿Volver al aspecto y lugar originales?",
  resetLookCurrent:
    "El aspecto y el lugar actuales vuelven a los ajustes originales de la aplicación. Otros perfiles, el número de hablantes, los modelos y el idioma no cambian. No se puede deshacer.",
  resetLookProfile:
    "El aspecto y el lugar del perfil {name} vuelven a los ajustes originales de la aplicación. Otros perfiles, el número de hablantes, los modelos y el idioma no cambian. No se puede deshacer.",
  resetLookConfirm: "Volver al original",
  dialogCancel: "Cancelar",
  fontFamily: "Familia de fuente",
  fontSearch: "Buscar fuentes",
  fontHint: "Se usa para todos los hablantes. Escribe para acotar la lista.",
  fontLoading: "Leyendo las fuentes instaladas",
  fontNoMatch: "Ninguna fuente instalada coincide",
  fontNotInstalled:
    "{font} no está instalada en este ordenador. La vista previa usa una sustituta y los archivos de subtítulos conservan este nombre.",
  downloadAllLabel: "Descargar lo que falta",
  downloadAll: "{label}: {count} ({size})",
  resetModels: "Restablecer los modelos y volver a descargarlos",
  resetModelsTitle: "¿Restablecer los modelos y volver a descargarlos?",
  resetModelsDescription:
    "Se elimina todo lo que esta aplicación descargó en {path} y se vuelven a descargar los modelos que necesita este ordenador. Las carpetas que añadiste nunca se tocan. No se puede deshacer.",
  resetModelsConfirm: "Eliminar y volver a descargar",
  videoFilterName: "Vídeos",
  readinessMissing:
    "No hay ningún modelo disponible para: {roles}. Descarga uno en Modelos o añade una carpeta que ya lo tenga.",
  roleTranscribe: "transcripción",
  roleDiarize: "separación de hablantes",
  roleFace: "detección de caras",
  roleRuntime: "entorno de ejecución",
  jobQueued: "En espera",
  jobCancelled: "Cancelado",
  jobCancel: "Cancelar {name}",
  jobRelayout: "Volver a escribir {name} con la posición y los colores actuales",
  jobDone: "Terminado: {cues} subtítulos, {speakers} hablantes",
  jobDoneOneSpeaker: "Terminado: {cues} subtítulos, 1 hablante",
  jobFaceless: "Los subtítulos sin cara detectada se colocaron en la parte inferior del vídeo.",
  jobSpeakersMissing:
    "El modelo de hablantes no detectó a nadie en este vídeo, así que todos los subtítulos tienen el mismo color.",
  jobFontMissing:
    "La fuente elegida no está en este ordenador, así que el ancho de los subtítulos es una estimación.",
  jobNoPercent: "este modelo no indica un porcentaje",
  speakerCount: "Número de hablantes",
  subtitleFormat: "Formato del archivo de subtítulos",
  speakerCountAuto: "Detectar automáticamente",
  speakerCountValue: "{count} hablantes",
  speakerCountOne: "1 hablante",
  jobFewerSpeakers:
    "Se indicaron {requested} hablantes, pero solo se encontraron {found}. Puede ocurrir con grabaciones cortas o voces parecidas.",
  jobStep: "Paso {number} de {total}: {stage}",
  jobEstimating: "Calculando el tiempo restante",
  jobRemaining: "Quedan unos {minutes} min",
  jobRemainingUnderMinute: "Queda menos de un minuto",
  stepStates: { done: "listo", current: "en curso", pending: "en espera" },
  outputPurposes: {
    drt: "Subtítulos Text+ para DaVinci Resolve, con colores, bordes, bordes exteriores y sombras",
    ttml: "Subtítulos con estilo (color, contorno, posición)",
    srt: "Subtítulos con palabras y tiempos (puede que sin colores; sin bordes ni posiciones)",
    ass: "Subtítulos con tiempos, colores, contorno y posición (no todas las aplicaciones usan todo)",
    fcpxml: "Línea de tiempo con títulos",
    json: "Datos de transcripción para reescribir",
  },
  jobProgressLabel: "Progreso de {name}",
  jobOutputs: "Archivos escritos",
  stages: {
    convert_model: "Convirtiendo el modelo",
    extract_audio: "Extrayendo el audio",
    transcribe: "Transcribiendo",
    diarize: "Separando hablantes",
    separate: "Separando las voces superpuestas",
    retranscribe: "Transcribiendo las voces separadas",
    faces: "Detectando caras",
    export: "Escribiendo los archivos de subtítulos",
  },
  positionHeading: "Posición de los subtítulos",
  positionLabel: "Colocar los subtítulos",
  anchorFaceAbove: "Encima de la cara",
  anchorFaceBelow: "Debajo de la cara",
  anchorVideoTop: "Parte superior del vídeo",
  anchorVideoBottom: "Parte inferior del vídeo",
  offsetX: "Ajuste fino izquierda-derecha (porcentaje del ancho)",
  offsetY: "Ajuste fino arriba-abajo (porcentaje del alto)",
  safeZoneHint:
    "Define el ancho de los bordes izquierdo y derecho donde no se colocan subtítulos. La vista previa los muestra como bandas oscuras.",
  safeLeft: "Zona segura izquierda (porcentaje del ancho)",
  safeRight: "Zona segura derecha (porcentaje del ancho)",
  faceModel: "Modelo de detección de caras",
  speechLanguage: "Idioma hablado",
  languageAuto: "Detectar automáticamente",
  sizeHeading: "Tamaño de los subtítulos",
  sizeHint: "Los tamaños siguen automáticamente la resolución de cada vídeo. Son proporciones del vídeo.",
  fontSize: "Tamaño del texto (porcentaje del lado corto)",
  captionWidth: "Línea más larga (porcentaje del ancho)",
  captionLineHeight: "Interlineado (múltiplo del tamaño del texto)",
  contrastWarning:
    "{name}: el contraste entre relleno y contorno es de {ratio}:1, inferior al mínimo WCAG AA de 4,5:1. Elige colores más distintos.",
  previewHeading: "Vista previa",
  previewFrameLabel: "Tamaño de la vista previa",
  previewDescription:
    "Subtítulo de la vista previa al {x} % desde la izquierda y al {y} % desde arriba en el fotograma {size}.",
  previewFontPixels: "Tamaño del texto en esta resolución: {px} px.",
  previewSample: "Los subtítulos con contorno cambian de color con cada hablante",
  previewLandscape: "1920×1080",
  previewVertical: "Vertical 1080×1920",
  previewUhd: "4K",
  speakersHeading: "Colores de los hablantes (hasta {count} hablantes)",
  speakersHint:
    "Elige un hablante para cambiar sus colores y grosores. La vista previa muestra sus subtítulos. El grosor y la posición son porcentajes del tamaño del texto.",
  speakerName: "Hablante {number}",
  colorColumn: "Color",
  fillWidth: "Grosor del relleno (porcentaje del tamaño del texto)",
  outlineWidth: "Grosor del borde (porcentaje del tamaño del texto)",
  rimWidth: "Grosor del borde exterior (porcentaje del tamaño del texto)",
  layerWidth: "Grosor (porcentaje del tamaño del texto)",
  colorTransparent: "Transparente",
  colorCustom: "Color libre",
  colorNames: {
    white: "Blanco",
    black: "Negro",
    red: "Rojo",
    orange: "Naranja",
    yellow: "Amarillo",
    green: "Verde",
    blue: "Azul",
    purple: "Morado",
  },
  fillLabel: "Relleno",
  outlineLabel: "Borde",
  rimLabel: "Borde exterior",
  formatLimits: {
    fcpxml: "FCPXML no muestra sombra. Elige DRT para usarla.",
    ttml: "TTML no muestra borde exterior ni sombra. Elige DRT para usarlos.",
    srt: "SRT solo conserva el color del texto: sin borde, borde exterior ni sombra. Elige DRT para usarlos.",
    ass: "ASS no tiene borde exterior ni sombra. CapCut lee solo el texto y los tiempos.",
  },
  shadowLabel: "Sombra",
  shadowColor: "Color de la sombra",
  shadowX: "Sombra izquierda-derecha (porcentaje del tamaño del texto)",
  shadowXShort: "Sombra izquierda-derecha",
  shadowY: "Sombra arriba-abajo (porcentaje del tamaño del texto; negativo hacia abajo)",
  shadowYShort: "Sombra arriba-abajo",
  shadowBlur: "Desenfoque de la sombra (0 a 50)",
  shadowBlurShort: "Desenfoque de la sombra",
  addSpeaker: "Añadir un hablante",
  removeSpeaker: "Quitar un hablante",
  resetPalette: "Restaurar los colores iniciales",
  modelsHeading: "Modelos",
  noRecommendedSpeakerModel:
    "No hay ningún modelo de separación de hablantes recomendado para este ordenador. Aun así puedes crear subtítulos para 1 hablante.",
  modelsHint:
    "Los modelos adecuados para este ordenador se marcan como recomendados. Los modelos que ya descargaste se encuentran y se reutilizan.",
  addModelFolder: "Añadir una carpeta con modelos existentes",
  modelFolders: "Carpetas analizadas",
  badgeRecommended: "Recomendado aquí",
  badgePresent: "Disponible",
  download: "Descargar {name} ({size})",
  downloadCancel: "Cancelar la descarga de {name}",
  downloadProgress: "{done} de {total}",
  downloadProgressLabel: "Descarga de {name}",
  deleteModel: "Eliminar la copia descargada de {name}",
  sourceLabel: "Origen: {url}",
  locationLabel: "Ubicación: {path}",
  loadFailed: "Algo salió mal: {detail}",
  errors: {
    no_video_stream: "Este archivo no tiene vídeo. Elige un archivo de vídeo.",
    no_audio_stream: "Este vídeo no tiene sonido, así que no hay nada que transcribir.",
    unreadable_size: "No se pudo leer el tamaño de este vídeo. Es posible que el archivo esté dañado.",
    unreadable_duration: "No se pudo leer la duración de este vídeo. Es posible que el archivo esté dañado.",
    no_words: "No se encontraron palabras en el audio.",
    model_missing: "{model} no está instalado. Descárgalo en Modelos.",
    no_model_for_role: "No hay ningún modelo instalado para {role}. Descarga uno en Modelos.",
    needs_memory:
      "{model} necesita {required} GB de {memory}; este ordenador tiene {available} GB. Elige un modelo más pequeño.",
    transcript_missing:
      "{file} no tiene una transcripción al lado, así que no se puede reescribir. Conserva el archivo .transcript.json con el mismo nombre en la misma carpeta.",
    profile_exists: "Ya existe un perfil llamado {name}. Elige otro nombre o sobrescríbelo desde la lista.",
    profile_missing: "El perfil {name} ya no existe.",
    models_busy:
      "No se pueden restablecer los modelos mientras hay una tarea o una descarga en curso. Espera a que termine o cancela e inténtalo de nuevo.",
    models_missing:
      "Los vídeos no se añadieron porque no hay modelo disponible para: {roles}. Descárgalos en Modelos o añade una carpeta que ya los tenga.",
    speaker_models_missing:
      "Desde 2 hablantes, o con la detección automática, hacen falta estos modelos: {roles}. Descárgalos en Modelos, o pon el número de hablantes en 1 y vuelve a añadir el vídeo.",
    update_busy:
      "No se puede actualizar mientras se procesan vídeos o se descargan modelos. Espera a que terminen y vuelve a intentarlo.",
    no_supported_files:
      "Este archivo no se puede añadir. Elige un vídeo (.mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts) o un archivo de subtítulos (.drt .fcpxml .srt .ttml .transcript.json).",
    profile_invalid:
      "{file} no se puede leer como perfil. Elige un archivo que haya exportado esta aplicación.",
    internal: "{detail}",
    cancelled: "Cancelado",
  },
  modelNotes: {
    "whisper-large-v3-turbo": "Transcripción. El mismo modelo funciona en Mac (Metal) y en Windows (Vulkan).",
    "nemotron-3.5-speaker":
      "Un codificador que solo sirve para distinguir hablantes. Funciona en la GPU o la CPU. Whisper turbo hace la transcripción. Un modelo completo que ya esté en tus carpetas también funciona.",
    "mossformer2-ss-16k-coreml":
      "Separa las voces solapadas en la GPU del Mac con Core ML. Lo usa «Alta calidad».",
    "mossformer2-ss-16k": "Separa las voces que se solapan. Lo usa «Alta calidad».",
    yunet: "Caras reales. Incluido con la app; da el mismo resultado en Mac y Windows.",
    "avatar-face-yolo":
      "Caras de avatares. Funciona cuando el archivo ONNX exportado está en una carpeta de modelos.",
    "geforce-runtime":
      "Opcional. Acelera la separación de hablantes en la GPU. Sin ella se ejecuta en la CPU, y vuelve a la CPU si la GPU falla.",
  },
  help: {
    version: "Versión {version}",
    intro:
      "Esta aplicación convierte lo que se dice en un vídeo en subtítulos, con un color para cada persona. Puedes usarlos en un programa para editar vídeo.",
    sections: {
      start: {
        heading: "Para empezar",
        parts: [
          {
            heading: "Instalar la aplicación",
            intro: "Descarga el archivo adecuado para tu ordenador e instálalo.",
            steps: [
              "Elige en la lista de abajo el archivo para tu ordenador.",
              "En Mac, abre el zip y pon la aplicación en la carpeta «Aplicaciones».",
              "En Windows, abre el exe y sigue los pasos.",
            ],
            notes: [
              "Mac: Transcribe-Captions-macos-arm64.zip",
              "Windows con GeForce: Transcribe-Captions-windows-geforce-x64-setup.exe",
              "Windows con Radeon: Transcribe-Captions-windows-radeon-x64-setup.exe",
              "Windows sin ninguna de las dos: sirve cualquier exe. Solo usa la CPU, así que tarda más.",
              "Si el Mac no te deja: abre Ajustes del Sistema > Privacidad y seguridad y pulsa «Abrir igualmente».",
              "Si Windows no te deja: pulsa «Más información» y luego «Ejecutar de todas formas».",
            ],
          },
          {
            heading: "Descargar los modelos",
            intro:
              "Los modelos son los datos que sirven para pasar la voz a texto. Solo hay que descargarlos una vez.",
            steps: [
              "Abre la pestaña «{tabModels}».",
              "Pulsa «{downloadAllLabel}».",
              "Espera a que la barra de cada modelo llegue al final.",
            ],
            notes: [
              "Un modelo listo muestra «{badgePresent}».",
              "Si no falta nada, este botón no aparece.",
              "Necesitas Internet. Los archivos son grandes, así que tarda un poco.",
              "Si una descarga se para, pulsa el botón otra vez. Sigue desde donde se paró.",
            ],
          },
          {
            heading: "Usar modelos que ya tienes",
            intro: "Si ya descargaste modelos, no hace falta descargarlos otra vez.",
            steps: [
              "Abre la pestaña «{tabModels}».",
              "Pulsa «{addModelFolder}».",
              "Elige la carpeta donde están los modelos.",
            ],
            notes: ["Los modelos encontrados muestran «{badgePresent}»."],
          },
        ],
      },
      transcribe: {
        heading: "Hacer subtítulos de un vídeo",
        parts: [
          {
            heading: "Añadir un vídeo",
            intro: "Cuando añades un vídeo, la aplicación empieza a hacer los subtítulos.",
            steps: [
              "Abre la pestaña «{tabJobs}».",
              "«{speakerCount}» empieza en 1. Si hablan más personas, elige cuántas. Si no lo sabes, elige «{speakerCountAuto}».",
              "En «{subtitleFormat}», elige el tipo de archivo. Al principio es DRT. Debajo se indica para qué sirve.",
              "Arrastra el vídeo al recuadro y suéltalo. Arrastrar es mantener pulsado mientras mueves. También puedes pulsar «{pickVideos}».",
            ],
            notes: [
              "El «?» junto al número explica para qué sirve.",
              "Si solo habla una persona, deja 1. Es más rápido.",
              "1 hablante es ligero: solo se ejecuta Whisper. La detección automática y 2 o más hablantes son carga alta: también se ejecutan Nemotron y MossFormer2, por lo que tarda mucho más y usa mucha más RAM o memoria de GPU.",
              "Puedes añadir estos vídeos: .mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts. Archivos de subtítulos: .drt .fcpxml .srt .ttml .transcript.json. Con otro archivo, la aplicación dice que no se puede añadir.",
              "Si añades varios vídeos, se hacen uno por uno.",
              "La primera vez, la aplicación también pasa un modelo a una forma que puede usar.",
              "Si el idioma de los subtítulos no es el correcto, elígelo en «{speechLanguage}», en la pestaña «{tabStyle}».",
            ],
          },
          {
            heading: "Ver cómo va",
            intro: "Después de añadir el vídeo, aparece una lista bajo el recuadro.",
            steps: [],
            notes: [
              "Muestra el paso actual y cuántos pasos hay.",
              "La barra muestra cómo va todo el trabajo.",
              "También indica más o menos cuánto falta.",
              "Para parar, pulsa el botón junto al nombre del vídeo.",
            ],
          },
          {
            heading: "Los archivos que se crean",
            intro:
              "Aparecen dos archivos en la carpeta del vídeo: el de subtítulos del tipo elegido y un archivo .transcript.json. Los anteriores se conservan. Si el nombre ya existe, los nuevos llevan -2 en el nombre.",
            steps: [],
            notes: [
              ".drt: subtítulos Text+ para DaVinci Resolve, con colores, bordes, bordes exteriores y sombras.",
              ".fcpxml: los subtítulos junto con el vídeo, para programas de edición.",
              ".ttml: subtítulos con colores y posiciones.",
              ".srt: subtítulos con palabras y tiempos. Puede que no se vean los colores. No lleva bordes ni posiciones.",
              ".ass: subtítulos para aplicaciones como CapCut. Incluyen colores, contorno y posición; que se vean depende de la aplicación.",
              ".transcript.json: los datos para volver a escribir los subtítulos. Consérvalo.",
            ],
          },
          {
            heading: "Cuando hablan 2 o más personas",
            intro:
              "Con la detección automática o con 2 o más hablantes se ejecutan Whisper turbo, el codificador Nemotron 3.5 y MossFormer2. Whisper turbo siempre hace la transcripción; Nemotron solo distingue a los hablantes.",
            steps: [],
            notes: [
              "Las voces a la vez aparecen en varias líneas. En un Mac, {memoryUnified}: {encoderGb} GB para el codificador Nemotron 3.5, unos {separateCoreMlGb} GB (estimación) para MossFormer2 con Core ML. Con 3 o más voces a la vez se separa dos veces, lo que tarda más.",
              "En un Mac, «{separationBackend}» permite elegir la CPU o la GPU del Mac para la separación de voces.",
              "Codificador Nemotron 3.5: por hora de audio, unos {encoderGpuSeconds} segundos en la GPU de un Mac, unos {encoderCpuSeconds} segundos en la CPU. Separación de voces, medida en un vídeo de cerca de una hora: unos {separateCoreMlMinutes} min con Core ML, unos {separateCpuMinutes} min en la CPU. Depende de cuánto del vídeo haya que separar.",
              "{speakerFiguresNote}",
            ],
          },
        ],
      },
      editor: {
        heading: "Abrir en el programa de edición",
        parts: [
          {
            heading: "Importar en DaVinci Resolve",
            intro:
              "Un archivo .drt importa los subtítulos como clips Text+, con sus colores, bordes, bordes exteriores y sombras.",
            steps: [
              "Abre un proyecto en DaVinci Resolve.",
              "En el menú, elige File > Import > Timeline...",
              "Elige el archivo .drt que hizo la aplicación.",
              "En la pantalla de confirmación, pulsa Import.",
              "Los subtítulos llegan como clips Text+. Selecciona un clip: en el Inspector, Text cambia las palabras y Shading los colores y los bordes.",
            ],
            notes: [
              "Los números de Select Element: 1 es el relleno, 2 el borde, 4 el borde exterior, 3 la sombra, 5 el grosor del relleno.",
              "En un proyecto nuevo, «Automatically set project setting» puede seguir activado. Si añades a un proyecto en curso, desactívalo para que la resolución no cambie.",
              "No muevas el archivo de vídeo.",
            ],
          },
          {
            heading: "Importar un archivo FCPXML",
            intro:
              "Un archivo FCPXML trae el vídeo y los subtítulos a la vez, con sus colores, bordes y posiciones.",
            steps: [
              "Abre un proyecto en tu programa de edición. Un proyecto guarda todo tu trabajo de edición.",
              "En el menú, importa la línea de tiempo (por ejemplo, File > Import > Timeline).",
              "Elige el archivo .fcpxml que hizo la aplicación.",
              "El vídeo y los subtítulos aparecen en una línea de tiempo. Es el lugar donde el vídeo se ordena por tiempo.",
            ],
            notes: [
              "Cada subtítulo tiene 2 o 3 capas una sobre otra. Para cambiar las palabras, cambia todas las capas.",
            ],
          },
          {
            heading: "Abrir un archivo ASS en CapCut",
            intro:
              "CapCut lee el texto y los tiempos de un archivo .ass. Las voces a la vez aparecen en varias líneas.",
            steps: [
              "Elige ASS en «{subtitleFormat}» y crea los subtítulos.",
              "Abre un proyecto en CapCut, pon el vídeo en la línea de tiempo y lleva el cabezal al principio.",
              "En la pestaña Subtítulos, elige Añadir subtítulos, luego Importar archivo, y selecciona el archivo .ass.",
              "Señala el archivo .ass en la lista y pulsa +. Los subtítulos empiezan en el cabezal.",
            ],
            notes: [
              "CapCut no usa los colores, contornos ni posiciones de cada hablante. Ajústalos en Texto dentro de CapCut (comprobado con CapCut 9.5.0 para Mac).",
              "Los nombres en la pantalla de CapCut pueden cambiar según el idioma y la versión.",
            ],
            link: {
              label: "Abrir la ayuda de CapCut",
              url: "https://www.capcut.com/es-es/help/how-to-import-subtitles",
            },
          },
          {
            heading: "Cuándo usar los otros tipos",
            intro: "Para otro tipo, elígelo en «{subtitleFormat}» antes de añadir el vídeo.",
            steps: [],
            notes: [
              "Para traer un archivo .ttml, elige Import Subtitle en el Media Pool. El Media Pool es donde se guardan tus vídeos y subtítulos.",
              "Usa el archivo .srt para corregir palabras o tiempos dentro de tu programa de edición. Si esta aplicación vuelve a escribir los subtítulos, tus correcciones no estarán.",
              "No uses FCPXML y TTML en la misma línea de tiempo. Los subtítulos se verían dos veces.",
            ],
          },
        ],
      },
      look: {
        heading: "Cambiar el aspecto y el lugar de los subtítulos",
        parts: [
          {
            heading: "Cambiar el aspecto y el lugar",
            intro:
              "Los cambios se ven enseguida en la vista previa y se guardan solos. La vista previa muestra cómo quedarán los subtítulos.",
            steps: [
              "Abre la pestaña «{tabStyle}».",
              "En «{positionHeading}», elige «{anchorFaceAbove}», «{anchorFaceBelow}», «{anchorVideoTop}» o «{anchorVideoBottom}».",
              "Ajusta el tamaño del texto con el control deslizante. Es un tirador que se mueve a izquierda y derecha. También puedes escribir un número.",
              "Elige un hablante y ajusta el relleno, el borde, el borde exterior y la sombra. Pulsa un cuadrado de color para elegir un color. «Transparente» quita el borde exterior o la sombra.",
            ],
            notes: [
              "La vista previa está a la derecha en una pantalla ancha y arriba en una estrecha. Sigue visible al desplazarte.",
              "Para alejar los subtítulos de los bordes izquierdo y derecho, ajusta las zonas seguras en «{positionHeading}». Las bandas oscuras de la vista previa las muestran.",
            ],
          },
          {
            heading: "Elegir una fuente",
            intro:
              "La fuente es la forma de las letras. Eliges entre las de este ordenador, y todas las personas usan la misma.",
            steps: [
              "Debajo de «{fontFamily}», pulsa donde aparece el nombre de la fuente.",
              "Escribe parte del nombre. La lista se hace más corta.",
              "Pulsa la fuente que quieras.",
            ],
            notes: [],
          },
          {
            heading: "Guardar y cargar un perfil",
            intro: "Un perfil son ajustes de aspecto y lugar guardados con un nombre.",
            steps: [
              "Escribe un nombre.",
              "Pulsa «{profileSave}».",
              "Para usarlo más tarde, pulsa su botón de cargar en la lista.",
            ],
            notes: [
              "La aplicación pregunta antes de sobrescribir o eliminar un perfil.",
              "Los modelos y el idioma hablado no se guardan en el perfil.",
            ],
          },
          {
            heading: "Usar un perfil en otro ordenador",
            intro: "Exporta un perfil a un archivo y podrás usarlo en otro ordenador.",
            steps: [
              "En la lista, pulsa el botón de exportar y elige dónde guardar.",
              "Copia el archivo al otro ordenador.",
              "En el otro ordenador, pulsa «{profileImport}» y elige el archivo.",
            ],
            notes: [
              "Un perfil importado aparece en la lista.",
              "Si el nombre ya existe, entra con otro nombre y no reemplaza nada.",
            ],
          },
          {
            heading: "Volver al aspecto original",
            intro: "Puedes devolver el aspecto y el lugar a los ajustes originales de la aplicación.",
            steps: ["Abre la pestaña «{tabStyle}».", "Pulsa «{resetLook}».", "Lee el mensaje y continúa."],
            notes: [
              "Si usas un perfil, solo vuelve ese perfil.",
              "El número de hablantes, los modelos, el idioma y otros perfiles no cambian.",
            ],
          },
        ],
      },
      rewrite: {
        heading: "Dar otro aspecto a subtítulos terminados",
        parts: [
          {
            heading: "Añadir un archivo de subtítulos",
            intro:
              "Puedes cambiar el aspecto sin rehacer los subtítulos desde el sonido. Suele ser rápido. Tarda más si la aplicación tiene que buscar caras.",
            steps: [
              "Carga el perfil que quieras. Si no hay ninguno, se usan los ajustes actuales.",
              "Abre la pestaña «{tabJobs}».",
              "Arrastra un archivo .drt, .fcpxml, .srt, .ttml o .transcript.json al recuadro y suéltalo. También puedes pulsar «{pickSubtitles}».",
              "La lista bajo el recuadro muestra cómo va y el resultado.",
            ],
            notes: [
              "El botón de volver a escribir, junto al nombre de un vídeo en la lista, crea archivos nuevos con otro nombre. Los anteriores se conservan.",
              "Los archivos originales se conservan. Los nuevos llevan -2 u otro número en el nombre.",
              "El tipo del archivo nuevo depende de «{subtitleFormat}», no del archivo añadido.",
              "El archivo .transcript.json con el mismo nombre debe estar en la misma carpeta.",
              "Cuando usas un perfil, no se usan los cambios que no guardaste en ese perfil.",
            ],
          },
        ],
      },
      settings: {
        heading: "Ajustes y actualizaciones",
        parts: [
          {
            heading: "Los ajustes",
            intro:
              "La pestaña «{tabSettings}» muestra la versión de la aplicación. Lo que elijas se guarda para la próxima vez.",
            steps: [
              "Abre la pestaña «{tabSettings}».",
              "En «{uiLanguageLabel}», elige un idioma. La pantalla cambia al momento.",
              "En «{uiThemeLabel}», elige «{uiThemeLight}» o «{uiThemeDark}». «{uiThemeSystem}» sigue a tu ordenador.",
            ],
            notes: [
              "Al final de la pestaña, «{licensesOpen}» muestra las bibliotecas de la aplicación y sus licencias.",
            ],
          },
          {
            heading: "Actualizar la aplicación",
            intro:
              "Cuando sale una versión nueva, la aplicación la muestra arriba de la ventana. Nada cambia hasta que pulses el botón.",
            steps: [
              "Para buscarla tú, pulsa «{updateCheck}» en la pestaña «{tabSettings}».",
              "Lee las novedades y pulsa «{updateInstall}».",
              "La aplicación descarga la actualización y se reinicia.",
            ],
            notes: [
              "No puedes actualizar mientras se procesan vídeos o se descargan modelos. Espera a que terminen.",
              "Buscar y descargar requieren conexión a Internet.",
            ],
          },
        ],
      },
      trouble: {
        heading: "Si algo no funciona",
        parts: [
          {
            heading: "Dice que falta un modelo",
            intro: "Sin los modelos necesarios, la aplicación no puede empezar los subtítulos.",
            steps: [
              "Abre la pestaña «{tabModels}».",
              "Pulsa «{downloadAllLabel}».",
              "Cuando termine, añade el vídeo otra vez.",
            ],
            notes: [],
          },
          {
            heading: "Los subtítulos para 2 o más hablantes no empiezan",
            intro:
              "La detección automática y 2 o más hablantes necesitan el codificador Nemotron 3.5 y MossFormer2, y al menos 8 GB de RAM.",
            steps: [],
            notes: [
              "En la pestaña «{tabModels}», descarga los modelos que faltan. Si no sirven para este ordenador, pulsa «{runWithOneSpeaker}».",
              "Con 1 hablante no hacen falta estos modelos, y todos los subtítulos tienen el mismo color.",
            ],
          },
          {
            heading: "Dice que no encontró ninguna cara",
            intro:
              "Sin cara, el subtítulo va abajo en el vídeo. Si eliges un lugar que no usa caras, los subtítulos siempre quedan en el mismo sitio.",
            steps: [
              "Abre la pestaña «{tabStyle}».",
              "En el lugar de los subtítulos, elige arriba o abajo del vídeo.",
              "Pulsa el botón de volver a escribir, junto al nombre del vídeo.",
            ],
            notes: [
              "Si no está en la lista, añade el archivo de subtítulos. Si usas un perfil, sobrescríbelo antes.",
              "Las caras de avatares necesitan otro modelo. Pon su archivo en una carpeta de modelos y elígelo en «{faceModel}».",
            ],
          },
          {
            heading: "Quieres empezar de nuevo",
            intro: "Si un modelo puede estar dañado, puedes descargarlo otra vez.",
            steps: ["Abre la pestaña «{tabModels}».", "Pulsa «{resetModels}».", "Lee el mensaje y continúa."],
            notes: [
              "Solo se borra lo que descargó esta aplicación. Las carpetas que añadiste se conservan.",
              "No se puede hacer mientras se crean subtítulos o hay una descarga.",
              "Después de borrar, la descarga de los modelos que faltan empieza sola.",
            ],
          },
          {
            heading: "El programa de edición no encuentra el vídeo",
            intro:
              "Un archivo .drt/FCPXML guarda el lugar y el nombre del vídeo. Si mueves el vídeo o le cambias el nombre, el programa de edición no lo encuentra.",
            steps: [
              "Devuelve el vídeo a su lugar de antes, con su nombre de antes.",
              "Abre otra vez el archivo .drt/.fcpxml.",
            ],
            notes: [],
          },
        ],
      },
    },
    imageFolder: "es",
    resolveFigures: {
      menu: "Elige File, luego Import y luego Timeline. 1 es File, 2 Import, 3 Timeline.",
      dialog: "La pantalla Import Timeline. 1 es Automatically set project setting, 2 el botón Import.",
      timeline: "La línea de tiempo después de importar. 1 son los clips Text+ en V2.",
      text: "El Inspector con un clip seleccionado. 1 es la pestaña Text, 2 donde cambias las palabras.",
      shading: "La pestaña Shading del Inspector. 1 es Shading, 2 los números de Select Element.",
    },
    images: {
      models: "La pestaña Modelos. 1 es la pestaña, 2 el botón de descarga, 3 la barra de cada modelo.",
      transcribe:
        "La pestaña de subtítulos. 1 es la pestaña, 2 el número de hablantes, 3 el tipo de archivo, 4 el recuadro para vídeos.",
      look: "La pestaña de aspecto y lugar. 1 es la pestaña, 2 el lugar de los subtítulos, 3 un control deslizante, 4 los colores de una persona.",
      font: "La lista de fuentes abierta. 1 muestra el nombre de la fuente, 2 es donde escribes, 3 la fuente que eliges.",
      profiles:
        "La zona de perfiles. 1 es donde escribes un nombre, 2 el botón de guardar, 3 el botón de cargar.",
      rewrite:
        "Después de añadir un archivo de subtítulos. 2 es la pestaña, 3 el recuadro para archivos, 4 la lista de resultados.",
    },
  },
};
