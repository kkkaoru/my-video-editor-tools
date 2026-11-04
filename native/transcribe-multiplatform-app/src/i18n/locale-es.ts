// Runs with bun: Spanish UI text.
import type { Messages } from "./messages.ts";

export const esMessages: Messages = {
  appName: "Transcribe for Resolve",
  tabJobs: "Transcribir",
  tabStyle: "Aspecto y posición de los subtítulos",
  tabHelp: "Cómo se usa",
  tabModels: "Modelos",
  noticeSpeakersOnCpu:
    "Distinguir a los hablantes se hace en la CPU, así que tarda más. Descarga los datos de GeForce en Modelos para que vaya más rápido.",
  noticeCpuOnly:
    "No se encontró ninguna GPU que pueda usar esta versión. Todo se hace en la CPU, así que tarda más.",
  dropTitle: "Suelta aquí vídeos o archivos de subtítulos",
  dropHint: "Los vídeos generan archivos de subtítulos para DaVinci Resolve, guardados junto a cada vídeo.",
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
  dialogCancel: "Cancelar",
  fontFamily: "Familia de fuente",
  fontSearch: "Buscar fuentes",
  fontHint: "Se usa para todos los hablantes. Escribe para acotar la lista.",
  fontLoading: "Leyendo las fuentes instaladas",
  fontNoMatch: "Ninguna fuente instalada coincide",
  fontNotInstalled:
    "{font} no está instalada en este ordenador. La vista previa usa una sustituta y los archivos de subtítulos conservan este nombre.",
  downloadAllLabel: "Descargar lo que falta",
  downloadAll: "{label}: {count} modelos ({size})",
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
  jobFaceless: "Los subtítulos sin cara detectada se colocaron en la parte inferior del vídeo.",
  jobNoPercent: "este modelo no indica un porcentaje",
  speakerCount: "Número de hablantes",
  speakerCountAuto: "Detectar automáticamente",
  speakerCountValue: "{count} hablantes",
  jobFewerSpeakers:
    "Se indicaron {requested} hablantes, pero solo se encontraron {found}. Puede ocurrir con grabaciones cortas o voces parecidas.",
  jobStep: "Paso {number} de {total}: {stage}",
  jobEstimating: "Calculando el tiempo restante",
  jobRemaining: "Quedan unos {minutes} min",
  jobRemainingUnderMinute: "Queda menos de un minuto",
  stepStates: { done: "listo", current: "en curso", pending: "en espera" },
  outputPurposes: {
    ttml: "Subtítulos con estilo (color, contorno, posición)",
    srt: "Subtítulos con palabras y tiempos (puede que sin colores; sin bordes ni posiciones)",
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
  faceModel: "Modelo de detección de caras",
  speechLanguage: "Idioma hablado",
  languageAuto: "Detectar automáticamente",
  sizeHeading: "Tamaño de los subtítulos",
  sizeHint: "Los tamaños siguen automáticamente la resolución de cada vídeo. Son proporciones del vídeo.",
  fontSize: "Tamaño del texto (porcentaje del lado corto)",
  captionWidth: "Línea más larga (porcentaje del ancho)",
  outlineWidth: "Grosor del contorno (porcentaje del texto)",
  captionLineHeight: "Interlineado (múltiplo del tamaño del texto)",
  rimWidth: "Borde negro fuera del contorno (porcentaje del texto)",
  unknownSpeaker: "Hablante desconocido",
  contrastWarning:
    "{name}: el contraste entre relleno y contorno es de {ratio}:1, inferior al mínimo WCAG AA de 4,5:1. Elige colores más distintos.",
  shadowEnabled: "Añadir sombra",
  shadowStrength: "Intensidad de la sombra (porcentaje)",
  previewHeading: "Vista previa",
  previewFrameLabel: "Tamaño de la vista previa",
  previewDescription:
    "Subtítulo de ejemplo al {x} % desde la izquierda y al {y} % desde arriba en el fotograma {size}.",
  previewFontPixels: "Tamaño del texto en esta resolución: {px} px.",
  previewSample: "Los subtítulos con contorno cambian de color con cada hablante",
  previewLandscape: "1920×1080",
  previewVertical: "Vertical 1080×1920",
  previewUhd: "4K",
  speakersHeading: "Colores de los hablantes (hasta {count} hablantes)",
  speakersHint: "Cada hablante recibe automáticamente un color de relleno y uno de contorno. Cámbialos aquí.",
  speakerName: "Hablante {number}",
  fillLabel: "relleno",
  outlineLabel: "contorno",
  addSpeaker: "Añadir un hablante",
  removeSpeaker: "Quitar un hablante",
  resetPalette: "Restaurar los colores iniciales",
  modelsHeading: "Modelos",
  modelsHint:
    "Los modelos adecuados para este ordenador se marcan como recomendados. Los modelos que ya descargaste se encuentran y se reutilizan.",
  addModelFolder: "Añadir una carpeta con modelos existentes",
  modelFolders: "Carpetas analizadas",
  badgeRecommended: "Recomendado aquí",
  badgePresent: "Disponible",
  useModel: "Usar {name}",
  inUseModel: "En uso: {name}",
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
    needs_nvidia: "{model} necesita una GPU NVIDIA. Elige otro modelo en Modelos.",
    needs_memory:
      "{model} necesita {required} GB de memoria; este ordenador tiene {available} GB. Elige un modelo más pequeño.",
    too_long_for_model:
      "{model} admite grabaciones de hasta {limit} minutos. Este video es más largo. Elige Nemotron 3 Diarization en Modelos.",
    transcript_missing:
      "{file} no tiene una transcripción al lado, así que no se puede reescribir. Conserva el archivo .transcript.json con el mismo nombre en la misma carpeta.",
    profile_exists: "Ya existe un perfil llamado {name}. Elige otro nombre o sobrescríbelo desde la lista.",
    profile_missing: "El perfil {name} ya no existe.",
    models_busy:
      "No se pueden restablecer los modelos mientras hay una tarea o una descarga en curso. Espera a que termine o cancela e inténtalo de nuevo.",
    models_missing:
      "Los vídeos no se añadieron porque no hay modelo disponible para: {roles}. Descárgalos en Modelos o añade una carpeta que ya los tenga.",
    internal: "{detail}",
    cancelled: "Cancelado",
  },
  modelNotes: {
    "whisper-large-v3-turbo": "Transcripción. El mismo modelo funciona en Mac (Metal) y en Windows (Vulkan).",
    "nemotron-3.5-transcribe-iq4":
      "Separación de hablantes. Para Mac con 32 GB o más, GPU con 24 GB o más y CPU.",
    "nemotron-3.5-transcribe-nvfp4":
      "Separación de hablantes. Para GPU Blackwell como la serie GeForce RTX 50.",
    "nemotron-3.5-transcribe-q2m":
      "Separación de hablantes. Para GPU de 16 GB; cada grabación puede durar unos 7 minutos.",
    "nemotron-3.5-transcribe-q2l": "Separación de hablantes. Para Mac de 24 GB.",
    "nemotron-3.5-transcribe-q8":
      "Separación de hablantes. La mejor calidad, para Mac de 64 GB y GPU de 48 GB.",
    "nemotron-3-diarization":
      "Solo separación de hablantes, 107 MB. Hasta 8 hablantes; funciona con poca memoria.",
    yunet: "Caras reales. Incluido con la app; da el mismo resultado en Mac y Windows.",
    "avatar-face-yolo":
      "Caras de avatares. Funciona cuando el archivo ONNX exportado está en una carpeta de modelos.",
    "geforce-runtime":
      "Versión CUDA de la herramienta de separación de hablantes. Sin ella, la separación se ejecuta en la CPU.",
  },
  help: {
    version: "Versión {version}",
    intro:
      "Esta aplicación convierte lo que se dice en un vídeo en subtítulos, con un color para cada persona. Puedes usarlos en DaVinci Resolve, un programa para editar vídeo.",
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
              "Mac: Transcribe-for-Resolve-macos-arm64.zip",
              "Windows con GeForce: Transcribe-for-Resolve-windows-geforce-x64-setup.exe",
              "Windows con Radeon: Transcribe-for-Resolve-windows-radeon-x64-setup.exe",
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
              "En «{speakerCount}», elige cuántas personas hablan. Si no lo sabes, deja «{speakerCountAuto}».",
              "Arrastra el vídeo al recuadro y suéltalo. Arrastrar es mantener pulsado mientras mueves. También puedes pulsar «{pickVideos}».",
            ],
            notes: [
              "El «?» junto al número explica para qué sirve.",
              "Si solo habla una persona, elige 1. Es más rápido.",
              "Puedes añadir estos vídeos: .mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts. Con otros archivos no pasa nada.",
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
              "Aparecen cuatro archivos en la carpeta del vídeo. Los anteriores se conservan. Si el nombre ya existe, los nuevos llevan -2 en el nombre.",
            steps: [],
            notes: [
              ".fcpxml: los subtítulos junto con el vídeo, para DaVinci Resolve.",
              ".ttml: subtítulos con colores y posiciones.",
              ".srt: subtítulos con palabras y tiempos. Puede que no se vean los colores. No lleva bordes ni posiciones.",
              ".transcript.json: los datos para volver a escribir los subtítulos. Consérvalo.",
            ],
          },
        ],
      },
      resolve: {
        heading: "Abrir en DaVinci Resolve",
        parts: [
          {
            heading: "Empieza por el archivo FCPXML",
            intro:
              "Un archivo FCPXML trae el vídeo y los subtítulos a la vez, con sus colores, bordes y posiciones. Por eso se usa primero.",
            steps: [
              "Abre un proyecto en DaVinci Resolve. Un proyecto guarda todo tu trabajo de edición.",
              "En el menú, elige File > Import > Timeline.",
              "Elige el archivo .fcpxml que hizo la aplicación.",
              "El vídeo y los subtítulos aparecen en una línea de tiempo. Es el lugar donde el vídeo se ordena por tiempo.",
            ],
            notes: [
              "Cada subtítulo tiene 2 o 3 capas una sobre otra. Para cambiar las palabras, cambia todas las capas.",
            ],
          },
          {
            heading: "Cuándo usar los otros archivos",
            intro: "Los otros archivos solo se usan cuando hacen falta.",
            steps: [],
            notes: [
              "Para traer un archivo .ttml, elige Import Subtitle en el Media Pool. El Media Pool es donde se guardan tus vídeos y subtítulos.",
              "Usa el archivo .srt para corregir palabras o tiempos dentro de Resolve. Si esta aplicación vuelve a escribir los subtítulos, tus correcciones no estarán.",
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
            intro: "Los cambios se ven enseguida en el ejemplo y se guardan solos.",
            steps: [
              "Abre la pestaña «{tabStyle}».",
              "En «{positionHeading}», elige «{anchorFaceAbove}», «{anchorFaceBelow}», «{anchorVideoTop}» o «{anchorVideoBottom}».",
              "Ajusta el tamaño del texto y el grosor del borde con los controles deslizantes. Un control deslizante es un asa que mueves a izquierda y derecha. También puedes escribir un número.",
              "Elige el color del texto y del borde para cada persona.",
            ],
            notes: [],
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
              "Arrastra un archivo .fcpxml, .srt, .ttml o .transcript.json al recuadro y suéltalo. También puedes pulsar «{pickSubtitles}».",
              "La lista bajo el recuadro muestra cómo va y el resultado.",
            ],
            notes: [
              "El botón de volver a escribir, junto al nombre de un vídeo en la lista, reemplaza los archivos del mismo nombre. Los anteriores no se conservan.",
              "Los archivos originales se conservan. Los nuevos llevan -2 u otro número en el nombre.",
              "El archivo .transcript.json con el mismo nombre debe estar en la misma carpeta.",
              "Cuando usas un perfil, no se usan los cambios que no guardaste en ese perfil.",
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
            heading: "Resolve no encuentra el vídeo",
            intro:
              "Un archivo FCPXML guarda el lugar y el nombre del vídeo. Si mueves el vídeo o le cambias el nombre, Resolve no lo encuentra.",
            steps: [
              "Devuelve el vídeo a su lugar de antes, con su nombre de antes.",
              "Abre otra vez el archivo .fcpxml.",
            ],
            notes: [],
          },
        ],
      },
    },
  },
};
