# 下载 Tesseract 中英文词库到 src-tauri/resources/tessdata
# 用途：发布构建启用 ocr feature 前，先填充词库（CI / 打包机联网执行）。
$ErrorActionPreference = "Stop"
$dir = Join-Path $PSScriptRoot "..\src-tauri\resources\tessdata"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$base = "https://github.com/tesseract-ocr/tessdata/raw/main"
foreach ($f in "chi_sim.traineddata","eng.traineddata") {
  Write-Host "下载 $f ..."
  Invoke-WebRequest -Uri "$base/$f" -OutFile (Join-Path $dir $f)
}
Write-Host "完成：词库已就位 $dir"
