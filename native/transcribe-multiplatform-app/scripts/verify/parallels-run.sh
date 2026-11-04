#!/bin/bash
# Development only. Runs a PowerShell script in the logged-in Windows user's session (prlctl exec alone runs as SYSTEM). usage: vmrun.sh [--no-wait] < script
wait=1; [ "$1" = "--no-wait" ] && wait=0
b64="$(iconv -t UTF-16LE | { printf '\xff\xfe'; cat; } | base64)"
run="powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\Users\Public\vm-task.ps1 > C:\Users\Public\vm-out-$$.txt 2>&1 < NUL & echo done > C:\Users\Public\vm-done.txt"
prlctl exec "Windows 11" powershell -NoProfile -Command "\$t='TranscribeTest'; schtasks /end /tn \$t 2>\$null | Out-Null; Remove-Item C:\\Users\\Public\\vm-done.txt -EA SilentlyContinue; [IO.File]::WriteAllBytes('C:\\Users\\Public\\vm-task.ps1',[Convert]::FromBase64String('$b64')); [IO.File]::WriteAllText('C:\\Users\\Public\\vm-run.cmd','$run'); \$a=New-ScheduledTaskAction -Execute 'C:\\Users\\Public\\vm-run.cmd'; \$p=New-ScheduledTaskPrincipal -UserId 'kkk4oru' -LogonType Interactive; Register-ScheduledTask -TaskName \$t -Action \$a -Principal \$p -Force | Out-Null; Start-ScheduledTask -TaskName \$t" 2>&1 | iconv -f cp932 -t utf-8 -c | tail -3
[ $wait = 1 ] || exit 0
for _ in $(seq 1 "${VM_WAIT:-100}"); do
  sleep 5
  prlctl exec "Windows 11" cmd /c "if exist C:\\Users\\Public\\vm-done.txt echo FIN" 2>/dev/null | grep -q FIN && break
done
prlctl exec "Windows 11" cmd /c "type C:\\Users\\Public\\vm-out-$$.txt" 2>&1 | iconv -f cp932 -t utf-8 -c | tr -d '\r'
