// Runs with bun: Japanese UI text.
import type { Messages } from "./messages.ts";

export const jaMessages: Messages = {
  appName: "Transcribe Captions",
  tabJobs: "字幕を作る",
  tabStyle: "字幕の見た目と位置",
  tabSettings: "設定",
  uiLanguageLabel: "画面の言語",
  uiLanguageSystem: "システムに合わせる",
  uiThemeLabel: "表示モード",
  uiThemeSystem: "システムに合わせる",
  uiThemeLight: "通常",
  uiThemeDark: "ダーク",
  tabHelp: "使い方",
  tabModels: "モデル",
  noticeSpeakersOnCpu:
    "話す人を聞き分ける作業は、パソコンの計算の部品 (CPU) で動くので、時間がかかります。「モデル」のタブで「速く動かすためのデータ」をダウンロードすると、速くなります。",
  noticeCpuOnly:
    "絵や計算が速い部品 (GPU) のうち、この版で使えるものが見つかりません。パソコンの計算の部品 (CPU) で動くので、時間がかかります。",
  dropTitle: "動画か字幕ファイルをここに入れる",
  dropHint: "動画から、動画編集ソフトで使う字幕ファイルを作ります。動画と同じフォルダに保存します。",
  dropHover: "指を離すとファイルを入れます",
  pickVideos: "動画を選ぶ",
  pickSubtitles: "字幕ファイルを選ぶ",
  subtitleFilterName: "字幕ファイル",
  dropProfileHint:
    "字幕ファイルを入れると、保存したプロファイル {name} の見た目で、新しいファイルを作ります。",
  dropCurrentHint: "字幕ファイルを入れると、今の設定の見た目で、新しいファイルを作ります。",
  speakerCountHelpLabel: "話す人の数を選ぶ理由",
  speakerCountHelp:
    "字幕の色を分けるには、声を聞き分ける必要があります。人数を選ぶと、まちがいが減ります。1 人を選ぶと、聞き分けをしないので速く終わります。聞き分け用のモデルも要りません。",
  speakerCountOnlyOne:
    "Nemotron 3.5 のモデルがないので、1 人だけ選べます。「{tabModels}」のタブでダウンロードすると、人数を選べます。",
  profilesHeading: "プロファイル",
  profilesHint:
    "プロファイルは、字幕の見た目と位置の設定に名前を付けて保存したものです。あとで読み込むと元に戻せます。モデルと話す言葉の設定は入りません。",
  profileName: "プロファイル名",
  profileSave: "新しく保存",
  profileEmpty: "保存したプロファイルはまだありません。",
  profileSelected: "使用中",
  profileChanged: "保存していない変更あり",
  profileLoad: "{name} を読み込む",
  profileOverwrite: "{name} を上書き",
  profileOverwriteTitle: "{name} を上書きしますか?",
  profileOverwriteDescription: "保存した見た目を今の設定に置きかえます。元には戻せません。",
  profileDelete: "{name} を削除",
  profileDeleteTitle: "{name} を削除しますか?",
  profileDeleteDescription: "プロファイルを消します。今の設定と字幕ファイルは消えません。元には戻せません。",
  profileExport: "{name} を書き出す",
  profileImport: "ファイルから読み込む",
  profileFileFilter: "プロファイル",
  resetLook: "見た目と位置を最初に戻す",
  resetLookTitle: "見た目と位置を最初に戻しますか?",
  resetLookCurrent:
    "今の見た目と位置を、アプリの最初の設定に戻します。ほかのプロファイル、話す人の数、モデル、言葉の設定は変わりません。元には戻せません。",
  resetLookProfile:
    "プロファイル {name} の見た目と位置を、アプリの最初の設定に戻します。ほかのプロファイル、話す人の数、モデル、言葉の設定は変わりません。元には戻せません。",
  resetLookConfirm: "最初に戻す",
  dialogCancel: "やめる",
  fontFamily: "フォント(文字の形)",
  fontSearch: "フォントをさがす",
  fontHint: "話す人全員に同じフォントを使います。名前を入力すると一覧が短くなります。",
  fontLoading: "パソコンのフォントを読み込んでいます",
  fontNoMatch: "合うフォントがありません",
  fontNotInstalled:
    "{font} はこのパソコンにありません。プレビューは別のフォントで表示します。字幕ファイルにはこの名前を書きます。",
  downloadAllLabel: "足りないモデルをダウンロード",
  downloadAll: "{label}: {count} ({size})",
  resetModels: "モデルを消してダウンロードし直す",
  resetModelsTitle: "モデルを消してダウンロードし直しますか?",
  resetModelsDescription:
    "このアプリが {path} にダウンロードしたものをすべて消します。そのあと、このパソコンに必要なモデルをダウンロードします。追加したフォルダの中は消しません。元には戻せません。",
  resetModelsConfirm: "消してダウンロードする",
  videoFilterName: "動画",
  readinessMissing:
    "{roles}のモデルがありません。「モデル」のタブでダウンロードするか、モデルが入っているフォルダを追加してください。",
  roleTranscribe: "文字起こし",
  roleDiarize: "話す人の聞き分け",
  roleFace: "顔を見つける作業",
  roleRuntime: "速く動かすためのデータ",
  jobQueued: "順番待ち",
  jobCancelled: "やめました",
  jobCancel: "{name} をやめる",
  jobRelayout: "{name} を今の位置と色で作り直す",
  jobDone: "できました: 字幕 {cues} 個、話す人 {speakers} 人",
  jobDoneOneSpeaker: "できました: 字幕 {cues} 個、話す人 1 人",
  jobFaceless: "顔が見つからない場面の字幕は、動画の下に置きました。",
  jobFontMissing: "選んだフォントがこのパソコンにないので、字幕の幅はおおよその値で計算しました。",
  jobNoPercent: "このモデルは進み具合を数字で出せません",
  speakerCount: "話す人の数",
  subtitleFormat: "字幕ファイルの形式",
  speakerCountAuto: "自動で決める",
  speakerCountValue: "{count} 人",
  speakerCountOne: "1 人",
  jobFewerSpeakers:
    "{requested} 人を選びましたが、{found} 人しか聞き分けられませんでした。音声が短いときや、声が似ているときに起こります。",
  jobStep: "{number}/{total} 番目: {stage}",
  jobEstimating: "残りの時間を計算しています",
  jobRemaining: "残り約 {minutes} 分",
  jobRemainingUnderMinute: "残り 1 分より短い",
  stepStates: { done: "終わり", current: "作業中", pending: "待っています" },
  outputPurposes: {
    drt: "DaVinci Resolve に読み込む Text+ の字幕です。色、縁、外側の縁、シャドウがすべて入ります",
    ttml: "色と位置が入った字幕",
    srt: "文字と時間の字幕 (色は出ないことがあります。縁と位置は入りません)",
    fcpxml: "動画編集ソフトで読める、字幕入りのファイル",
    json: "字幕を作り直すためのデータ",
  },
  jobProgressLabel: "{name} の進み具合",
  jobOutputs: "できたファイル",
  stages: {
    convert_model: "モデルをこのアプリで使える形に直しています",
    extract_audio: "音声を取り出しています",
    transcribe: "文字起こしをしています",
    diarize: "話す人を聞き分けています",
    faces: "顔をさがしています",
    export: "字幕ファイルを作っています",
  },
  positionHeading: "字幕の位置",
  positionLabel: "字幕を置く場所",
  anchorFaceAbove: "顔の上",
  anchorFaceBelow: "顔の下",
  anchorVideoTop: "動画の上",
  anchorVideoBottom: "動画の下",
  offsetX: "左右の細かい調整 (動画の横はばに対する %)",
  offsetY: "上下の細かい調整 (動画の高さに対する %)",
  safeZoneHint: "動画の左右の端に、字幕を置かない幅を決めます。プレビューでは暗い帯で出ます。",
  safeLeft: "左のセーフゾーン (動画の横はばに対する %)",
  safeRight: "右のセーフゾーン (動画の横はばに対する %)",
  faceModel: "顔を見つけるモデル",
  speechLanguage: "話している言葉",
  languageAuto: "自動で決める",
  sizeHeading: "字幕の大きさ",
  sizeHint: "動画の大きさに合わせて自動で計算します。下の数字は動画に対する割合です。",
  fontSize: "文字の大きさ (動画の短い辺に対する %)",
  captionWidth: "1 行の長さの上限 (動画の横はばに対する %)",
  captionLineHeight: "行と行の間 (文字の大きさの何倍か)",
  unknownSpeaker: "不明な人",
  contrastWarning:
    "{name}: 文字と縁の色が近くて、読みにくいです (色の差は {ratio}:1、目安は 4.5:1 以上)。明るさのちがう色を選んでください。",
  previewHeading: "プレビュー",
  previewFrameLabel: "プレビューの大きさ",
  previewDescription: "プレビューの字幕は {size} の画面で、横 {x}%、縦 {y}% の位置にあります。",
  previewFontPixels: "この大きさの動画では、文字の大きさは {px} ピクセル (画面の点の数) です。",
  previewSample: "話す人ごとに色が変わる字幕です",
  previewLandscape: "1920×1080",
  previewVertical: "縦 1080×1920",
  previewUhd: "4K",
  speakersHeading: "話す人の色 (最大 {count} 人)",
  speakersHint:
    "話す人を選ぶと、その人の色と太さを変えられます。プレビューには、選んだ人の字幕が出ます。太さと位置は文字サイズに対する % です。",
  speakerName: "話す人 {number}",
  colorColumn: "色",
  fillWidth: "塗りの太さ (文字サイズに対する %)",
  outlineWidth: "縁の太さ (文字サイズに対する %)",
  rimWidth: "外側の縁の太さ (文字サイズに対する %)",
  layerWidth: "太さ (文字サイズに対する %)",
  colorTransparent: "透明",
  colorCustom: "自由な色",
  colorNames: {
    white: "白",
    black: "黒",
    red: "赤",
    orange: "オレンジ",
    yellow: "黄",
    green: "緑",
    blue: "青",
    purple: "紫",
  },
  fillLabel: "塗り",
  outlineLabel: "縁",
  rimLabel: "外側の縁",
  formatLimits: {
    fcpxml: "FCPXML ではシャドウは出ません。シャドウを使うときは DRT を選びます。",
    ttml: "TTML では外側の縁とシャドウは出ません。使うときは DRT を選びます。",
    srt: "SRT で効くのは文字の色だけです。縁、外側の縁、シャドウは出ません。使うときは DRT を選びます。",
  },
  shadowLabel: "シャドウ",
  shadowColor: "影の色",
  shadowX: "影の左右の位置 (文字サイズに対する %)",
  shadowXShort: "影の左右の位置",
  shadowY: "影の上下の位置 (文字サイズに対する %、マイナスが下)",
  shadowYShort: "影の上下の位置",
  shadowBlur: "影のぼかし (0〜50)",
  shadowBlurShort: "影のぼかし",
  addSpeaker: "話す人を増やす",
  removeSpeaker: "話す人を減らす",
  resetPalette: "最初の色に戻す",
  modelsHeading: "モデル",
  noRecommendedSpeakerModel:
    "このパソコンに合う、おすすめの Nemotron 3.5 のモデルがありません。話す人が 1 人の字幕は作れます。",
  modelsHint:
    "モデルは、文字起こしに使うデータです。このパソコンに合うモデルには「おすすめ」と出ます。ダウンロードしたことのあるモデルは、自動で見つけて使います。",
  addModelFolder: "持っているモデルのフォルダを追加",
  modelFolders: "さがすフォルダ",
  badgeRecommended: "おすすめ",
  badgePresent: "使えます",
  useModel: "{name} を使う",
  inUseModel: "使用中: {name}",
  download: "{name} をダウンロード ({size})",
  downloadCancel: "{name} のダウンロードをやめる",
  downloadProgress: "{done} / {total}",
  downloadProgressLabel: "{name} のダウンロード",
  deleteModel: "{name} のダウンロードしたものを消す",
  sourceLabel: "ダウンロード元: {url}",
  locationLabel: "場所: {path}",
  loadFailed: "問題が起きました: {detail}",
  errors: {
    no_video_stream: "このファイルには映像がありません。動画のファイルを選んでください。",
    no_audio_stream: "この動画には音声がありません。だから文字起こしはできません。",
    unreadable_size: "動画の大きさを読み取れません。ファイルがこわれているかもしれません。",
    unreadable_duration: "動画の長さを読み取れません。ファイルがこわれているかもしれません。",
    no_words: "音声の中に言葉が見つかりませんでした。",
    model_missing: "{model} がありません。「モデル」のタブでダウンロードしてください。",
    no_model_for_role: "{role}のモデルがありません。「モデル」のタブでダウンロードしてください。",
    needs_nvidia:
      "{model} は、NVIDIA の GPU (絵や計算が速い部品) がないと動きません。「モデル」のタブで別のモデルを選んでください。",
    needs_memory:
      "{model} を動かすには、メモリ (作業に使う場所) が {required} GB 必要です。このパソコンは {available} GB です。小さいモデルを選んでください。",
    too_long_for_model:
      "{model} は {limit} 分までの音声に使えます。この動画は長すぎます。話す人の数を 1 人にしてください。",
    transcript_missing:
      "{file} と同じフォルダに、字幕を作り直すデータがありません。だから作り直せません。同じ名前の .transcript.json を同じフォルダに置いてください。",
    profile_exists:
      "{name} という名前のプロファイルはもうあります。別の名前にするか、一覧で上書きしてください。",
    profile_missing: "プロファイル {name} はもうありません。",
    models_busy:
      "字幕を作っている間とダウンロードの間は、モデルを消せません。終わるのを待つか、やめてから、もう一度試してください。",
    models_missing:
      "動画を入れられませんでした。{roles}のモデルがありません。「モデル」のタブでダウンロードするか、モデルが入っているフォルダを追加してください。",
    no_supported_files:
      "このファイルは入れられません。動画 (.mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts) か、字幕ファイル (.drt .fcpxml .srt .ttml .transcript.json) を入れてください。",
    profile_invalid:
      "{file} はプロファイルのファイルとして読めません。このアプリで書き出したファイルを選んでください。",
    internal: "{detail}",
    cancelled: "やめました",
  },
  modelNotes: {
    "whisper-large-v3-turbo": "文字起こしに使います。Mac でも Windows でも同じモデルです。",
    "nemotron-3.5-transcribe-iq4":
      "話す人の聞き分けに使います。メモリ 32 GB 以上の Mac、24 GB 以上の GPU、CPU 向けです。",
    "nemotron-3.5-transcribe-nvfp4":
      "話す人の聞き分けに使います。GeForce RTX 50 シリーズなどの GPU 向けです。",
    "nemotron-3.5-transcribe-q2m":
      "話す人の聞き分けに使います。16 GB の GPU 向けです。1 回の音声は 7 分くらいまでです。",
    "nemotron-3.5-transcribe-q2l": "話す人の聞き分けに使います。メモリ 24 GB の Mac 向けです。",
    "nemotron-3.5-transcribe-q8":
      "話す人の聞き分けに使います。一番ていねいです。メモリ 64 GB の Mac と 48 GB の GPU 向けです。",
    yunet: "人の顔を見つけます。アプリに入っています。",
    "avatar-face-yolo":
      "アバターの顔を見つけます。自分で用意したモデルのファイル (.onnx) をモデルのフォルダに置くと使えます。",
    "geforce-runtime": "話す人の聞き分けを GeForce で速く動かすためのデータです。ないときは CPU で動きます。",
  },
  help: {
    version: "バージョン {version}",
    intro:
      "このアプリは、動画の中で話している言葉を字幕にします。話す人ごとに字幕の色を変えます。作った字幕は、動画編集ソフトで使えます。",
    sections: {
      start: {
        heading: "はじめにすること",
        parts: [
          {
            heading: "アプリを入れる",
            intro: "パソコンに合うファイルをダウンロードして入れます。",
            steps: [
              "下の一覧から、パソコンに合うファイルを選びます。",
              "Mac は zip を開いて、アプリを「アプリケーション」のフォルダに入れます。",
              "Windows は exe を開いて、案内のとおりに進めます。",
            ],
            notes: [
              "Mac: Transcribe-Captions-macos-arm64.zip",
              "GeForce の Windows: Transcribe-Captions-windows-geforce-x64-setup.exe",
              "Radeon の Windows: Transcribe-Captions-windows-radeon-x64-setup.exe",
              "どちらもない Windows: どちらの exe でも使えます。パソコンの CPU だけで動くので、時間がかかります。",
              "Mac で止められたとき: システム設定 > プライバシーとセキュリティ を開いて、「このまま開く」を押します。",
              "Windows で止められたとき: 「詳細情報」を押して、「実行」を押します。",
            ],
          },
          {
            heading: "モデルをダウンロードする",
            intro:
              "モデルは、文字起こしに使うデータです。文字起こしは、話している言葉を文字にすることです。はじめに一度だけ入れます。",
            steps: [
              "「{tabModels}」のタブを開きます。",
              "「{downloadAllLabel}」のボタンを押します。",
              "モデルごとのバーが最後まで進むのを待ちます。",
            ],
            notes: [
              "終わったモデルには「{badgePresent}」と出ます。",
              "足りないモデルがないときは、このボタンは出ません。",
              "インターネットが必要です。大きいので時間がかかります。",
              "ダウンロードが止まったときは、もう一度押すと続きから進みます。",
            ],
          },
          {
            heading: "すでにあるモデルを使う",
            intro: "前にダウンロードしたモデルがあれば、もう一度ダウンロードしなくても使えます。",
            steps: [
              "「{tabModels}」のタブを開きます。",
              "「{addModelFolder}」のボタンを押します。",
              "モデルが入っているフォルダを選びます。",
            ],
            notes: ["見つかったモデルには「{badgePresent}」と出ます。"],
          },
        ],
      },
      transcribe: {
        heading: "動画から字幕を作る",
        parts: [
          {
            heading: "動画を入れる",
            intro: "動画を入れると、字幕を作り始めます。",
            steps: [
              "「{tabJobs}」のタブを開きます。",
              "「{speakerCount}」は、最初は 1 人です。2 人以上が話す動画は、人数を選びます。分からないときは「{speakerCountAuto}」を選びます。",
              "「{subtitleFormat}」で、作るファイルの形式を選びます。最初は DRT です。下に、その形式の使い道が出ます。",
              "動画を枠の中までドラッグして、指を離します。ドラッグは、押したまま動かすことです。「{pickVideos}」のボタンで選ぶこともできます。",
            ],
            notes: [
              "人数の横の「?」を押すと、人数を選ぶ理由が読めます。",
              "1 人しか話さない動画は「1 人」のままにします。速く終わります。",
              "人数を選べるのは、Nemotron 3.5 のモデルがあるときです。「{tabModels}」のタブでダウンロードできます。",
              "動画を入れられる形は .mp4 .mov .mkv .m4v .webm .avi .mts .mxf .ts です。字幕ファイルは .drt .fcpxml .srt .ttml .transcript.json です。ほかのファイルを入れると、入れられないと出ます。",
              "動画を何本も入れると、1 本ずつ順番に作ります。",
              "はじめて作るときは、モデルをこのアプリで使える形に直す作業が入ります。",
              "字幕の言葉がちがうときは、「{tabStyle}」のタブの「{speechLanguage}」で言葉を選びます。",
            ],
          },
          {
            heading: "進み具合を見る",
            intro: "動画を入れると、枠の下に動画の一覧が出ます。",
            steps: [],
            notes: [
              "今している作業と、全部で何番目かが出ます。",
              "バーは全体の進み具合です。",
              "残りの時間も出ます。これはおおよその時間です。",
              "途中でやめたいときは、動画の名前の横にあるボタンを押します。",
            ],
          },
          {
            heading: "できるファイル",
            intro:
              "動画と同じフォルダに、選んだ形式の字幕ファイルと .transcript.json の 2 つができます。前に作ったファイルは消しません。同じ名前があるときは、新しいファイルの名前に -2 を付けます。",
            steps: [],
            notes: [
              ".drt: DaVinci Resolve に読み込む Text+ の字幕です。色、縁、外側の縁、シャドウが入ります。",
              ".fcpxml: 動画編集ソフトで読める、字幕入りのファイルです。",
              ".ttml: 色と位置が入った字幕のファイルです。",
              ".srt: 文字と時間の字幕です。色は出ないことがあります。縁と位置は入りません。",
              ".transcript.json: 字幕を作り直すときに使うデータです。消さないでください。",
            ],
          },
        ],
      },
      editor: {
        heading: "動画編集ソフトに読み込む",
        parts: [
          {
            heading: "DaVinci Resolve で読み込む",
            intro:
              ".drt のファイルは、字幕を Text+ のクリップとして読み込みます。色、縁、外側の縁、シャドウも入ります。",
            steps: [
              "DaVinci Resolve でプロジェクトを開きます。",
              "メニューの File > Import > Timeline... を選びます。",
              "できた .drt のファイルを選びます。",
              "確認の画面で Import を押します。",
              "字幕は Text+ のクリップになります。クリップを選ぶと、Inspector の Text で文字を、Shading で色や縁を直せます。",
            ],
            notes: [
              "Select Element の番号は、1 が塗り、2 が縁、4 が外側の縁、3 がシャドウ、5 が塗りの太さです。",
              "新しいプロジェクトなら、Automatically set project setting はオンのままで大丈夫です。作業中のプロジェクトに足すときは、解像度が変わらないようにオフにします。",
              "動画のファイルは動かさないでください。",
            ],
          },
          {
            heading: "FCPXML を読み込む",
            intro:
              "FCPXML は、動画と字幕を一度に読み込めるファイルです。字幕の色、縁、位置も一緒に入ります。",
            steps: [
              "動画編集ソフトでプロジェクトを開きます。プロジェクトは、編集の作業をまとめたものです。",
              "メニューからタイムラインを読み込みます (例: File > Import > Timeline)。",
              "できた .fcpxml のファイルを選びます。",
              "タイムラインに動画と字幕が並びます。タイムラインは、動画を時間の順に並べる場所です。",
            ],
            notes: ["字幕は 2〜3 段に重なっています。文字を直すときは、全部の段を直します。"],
          },
          {
            heading: "ほかの形式を使うとき",
            intro: "ほかの形式は、動画を入れる前に「{subtitleFormat}」で選びます。",
            steps: [],
            notes: [
              ".ttml は、Media Pool で Import Subtitle を選んで読み込みます。Media Pool は、動画や字幕を入れておく場所です。",
              ".srt は、動画編集ソフトの中で文字や時間を直すときに使います。このアプリで作り直すと、直した文字は入りません。",
              "FCPXML と TTML を同じタイムラインで一緒に使わないでください。字幕が二重に見えます。",
            ],
          },
        ],
      },
      look: {
        heading: "字幕の見た目と位置を変える",
        parts: [
          {
            heading: "見た目と位置を変える",
            intro:
              "変えると、すぐにプレビューに出ます。プレビューは、字幕がどう見えるかを先に見る画面です。変えた内容は自動で保存します。",
            steps: [
              "「{tabStyle}」のタブを開きます。",
              "「{positionHeading}」で、「{anchorFaceAbove}」「{anchorFaceBelow}」「{anchorVideoTop}」「{anchorVideoBottom}」のどれかを選びます。",
              "スライダーで、文字の大きさを決めます。スライダーは、左右に動かすつまみです。数字を入力することもできます。",
              "話す人を選んで、塗り、縁、外側の縁、シャドウを決めます。色の四角を押すと色を選べます。外側の縁とシャドウは「透明」で消せます。",
            ],
            notes: [
              "プレビューは、広い画面では右に、せまい画面では上に出ます。下に動かしても見えたままです。",
              "字幕を動画の左右の端に置きたくないときは、「{positionHeading}」のセーフゾーンで幅を決めます。プレビューの暗い帯がその幅です。",
            ],
          },
          {
            heading: "フォントを選ぶ",
            intro:
              "フォントは、文字の形です。このパソコンに入っているフォントから選びます。話す人が何人いても、同じフォントを使います。",
            steps: [
              "「{fontFamily}」の下の、フォントの名前が出ているところを押します。",
              "フォントの名前の一部を入力します。一覧が短くなります。",
              "使いたいフォントを押します。",
            ],
            notes: [],
          },
          {
            heading: "プロファイルを保存して読み込む",
            intro: "プロファイルは、見た目と位置の設定に名前を付けて保存したものです。",
            steps: [
              "名前を入力します。",
              "「{profileSave}」のボタンを押します。",
              "あとで使うときは、一覧の読み込むボタンを押します。",
            ],
            notes: [
              "上書きと削除は、確認してから行います。",
              "モデルや話す言葉の設定は、プロファイルに入りません。",
            ],
          },
          {
            heading: "プロファイルを別のパソコンで使う",
            intro: "プロファイルをファイルに書き出すと、別のパソコンでも使えます。",
            steps: [
              "一覧の書き出すボタンを押して、保存する場所を選びます。",
              "できたファイルを、別のパソコンにコピーします。",
              "別のパソコンで「{profileImport}」のボタンを押して、そのファイルを選びます。",
            ],
            notes: [
              "読み込んだプロファイルは、一覧に出ます。",
              "同じ名前があるときは、上書きしないで、別の名前で入れます。",
            ],
          },
          {
            heading: "見た目と位置を最初に戻す",
            intro: "見た目と位置を、アプリの最初の設定に戻せます。",
            steps: [
              "「{tabStyle}」のタブを開きます。",
              "「{resetLook}」のボタンを押します。",
              "確認の画面を読んで、進めます。",
            ],
            notes: [
              "プロファイルを使っているときは、そのプロファイルだけ戻します。",
              "話す人の数、モデル、言葉の設定と、ほかのプロファイルは変わりません。",
            ],
          },
        ],
      },
      rewrite: {
        heading: "作った字幕を別の見た目に直す",
        parts: [
          {
            heading: "字幕ファイルを入れる",
            intro:
              "音声から作り直さなくても、字幕の見た目を直せます。ふつうはすぐに終わります。顔をさがす作業が入ると、時間がかかります。",
            steps: [
              "使いたいプロファイルを読み込みます。読み込んでいないときは、今の設定を使います。",
              "「{tabJobs}」のタブを開きます。",
              ".drt、.fcpxml、.srt、.ttml、.transcript.json のどれかを枠の中までドラッグして、指を離します。「{pickSubtitles}」のボタンで選ぶこともできます。",
              "枠の下の一覧に、進み具合と結果が出ます。",
            ],
            notes: [
              "一覧の、動画の名前の横にある作り直すボタンは、別の名前で新しいファイルを作ります。元のファイルは残ります。",
              "元のファイルは消しません。新しいファイルの名前に -2 などを付けます。",
              "新しいファイルの形式は、入れたファイルではなく「{subtitleFormat}」で決まります。",
              "同じフォルダに、同じ名前の .transcript.json が必要です。",
              "プロファイルを使っているときは、そのプロファイルに保存していない変更を使いません。",
            ],
          },
        ],
      },
      settings: {
        heading: "言語と表示モードを変える",
        parts: [
          {
            heading: "設定のタブ",
            intro:
              "「{tabSettings}」のタブには、アプリのバージョンが出ます。選んだ内容は、次に開いたときも残ります。",
            steps: [
              "「{tabSettings}」のタブを開きます。",
              "「{uiLanguageLabel}」で言語を選びます。画面がすぐ変わります。",
              "「{uiThemeLabel}」で「{uiThemeLight}」か「{uiThemeDark}」を選びます。「{uiThemeSystem}」にすると、パソコンの設定に合わせます。",
            ],
            notes: [],
          },
        ],
      },
      trouble: {
        heading: "うまくいかないとき",
        parts: [
          {
            heading: "モデルがないと出たとき",
            intro: "必要なモデルがないと、字幕を作り始められません。",
            steps: [
              "「{tabModels}」のタブを開きます。",
              "「{downloadAllLabel}」のボタンを押します。",
              "終わったら、もう一度動画を入れます。",
            ],
            notes: [],
          },
          {
            heading: "話す人の数を選べないとき",
            intro: "人数を選ぶには、Nemotron 3.5 のモデルが要ります。ないときは、1 人の字幕を作ります。",
            steps: [],
            notes: [
              "このパソコンに合う Nemotron 3.5 があるときは、「{tabModels}」のタブでダウンロードします。",
              "合うモデルがないときも、1 人の字幕は作れます。",
            ],
          },
          {
            heading: "顔が見つからないと出たとき",
            intro:
              "顔が見つからない場面の字幕は、動画の下に置きます。顔を使わない場所を選ぶと、いつも同じ場所に出せます。",
            steps: [
              "「{tabStyle}」のタブを開きます。",
              "字幕を置く場所で、動画の上か下を選びます。",
              "動画の名前の横にある、作り直すボタンを押します。",
            ],
            notes: [
              "一覧にないときは、字幕ファイルを入れます。プロファイルを使っているときは、先に上書きします。",
              "アバターの顔には、別のモデルが必要です。モデルのファイルをモデルのフォルダに置いて、「{faceModel}」で選びます。",
            ],
          },
          {
            heading: "はじめからやり直したいとき",
            intro: "モデルがこわれたかもしれないときは、入れ直せます。",
            steps: [
              "「{tabModels}」のタブを開きます。",
              "「{resetModels}」のボタンを押します。",
              "確認の画面を読んで、進めます。",
            ],
            notes: [
              "消すのは、このアプリがダウンロードしたものだけです。自分で追加したフォルダは消しません。",
              "字幕を作っている間とダウンロードの間は、できません。",
              "消したあと、足りないモデルのダウンロードが自動で始まります。",
            ],
          },
          {
            heading: "動画編集ソフトで動画が見つからないとき",
            intro:
              ".drt/FCPXML には、動画の場所と名前が書いてあります。動画を動かしたり名前を変えたりすると、見つかりません。",
            steps: ["動画を元の場所と元の名前に戻します。", "もう一度 .drt/.fcpxml を読み込みます。"],
            notes: [],
          },
        ],
      },
    },
    imageFolder: "ja",
    resolveFigures: {
      menu: "File、Import、Timeline の順に選びます。1 は File、2 は Import、3 は Timeline です。",
      dialog:
        "Import Timeline の確認の画面です。1 は Automatically set project setting、2 は Import のボタンです。",
      timeline: "読み込んだ後のタイムラインです。1 は V2 に並ぶ Text+ のクリップです。",
      text: "クリップを選んだ Inspector です。1 は Text のタブ、2 は文字を直す欄です。",
      shading: "Inspector の Shading のタブです。1 は Shading、2 は Select Element の番号です。",
    },
    images: {
      models: "「モデル」のタブの画面です。1 はタブ、2 はダウンロードのボタン、3 はモデルごとのバーです。",
      transcribe:
        "「字幕を作る」のタブの画面です。1 はタブ、2 は話す人の数、3 は字幕ファイルの形式、4 は動画を入れる枠です。",
      look: "「字幕の見た目と位置」のタブの画面です。1 はタブ、2 は字幕を置く場所、3 はスライダー、4 は話す人の色です。",
      font: "フォントの一覧を開いた画面です。1 はフォントの名前が出ているところ、2 は名前を入力するところ、3 は選ぶフォントです。",
      profiles:
        "プロファイルの画面です。1 は名前を入力するところ、2 は保存のボタン、3 は読み込むボタンです。",
      rewrite: "字幕ファイルを入れた画面です。2 はタブ、3 はファイルを入れる枠、4 は結果の一覧です。",
    },
  },
};
