# Retire Explorer receipts only after all six commands and the full receipt are
# published. A handled failure restores the previous binaries and receipts.
$script:EskaFullInstall = $null

function Get-EskaMessage($Russian, $English) {
  $culture = if ($env:LC_ALL) { $env:LC_ALL } else { $PSUICulture }
  if ($culture -like "ru*") { return $Russian }
  return $English
}

function Get-EskaItem($Path) {
  try { return Get-Item -LiteralPath $Path -Force -ErrorAction Stop }
  catch [System.Management.Automation.ItemNotFoundException] { return $null }
}

function Restore-EskaFile($Entry) {
  $item = Get-EskaItem $Entry.Path
  if ($null -ne $item -and ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint))) {
    throw "component-installation-path: $($Entry.Path)"
  }
  if (Test-Path -LiteralPath $Entry.Backup) {
    if ((Test-Path -LiteralPath $Entry.Path -PathType Leaf) -and
        ((Get-FileHash -LiteralPath $Entry.Path).Hash -eq (Get-FileHash -LiteralPath $Entry.Backup).Hash)) { return }
    Copy-Item -LiteralPath $Entry.Backup -Destination $Entry.Path -Force -ErrorAction Stop
  } elseif ($null -ne $item) {
    Remove-Item -LiteralPath $Entry.Path -Force -ErrorAction Stop
  }
}

function Stop-EskaFullInstall {
  $state = $script:EskaFullInstall
  if ($null -eq $state) { return }
  if ($state.Ready -and -not $state.Committed) {
    $restored = $true
    foreach ($entry in $state.Entries) {
      try { Restore-EskaFile $entry } catch { $restored = $false }
    }
    if (-not $restored) {
      throw (Get-EskaMessage `
        "component-installation-rollback: Не удалось восстановить прежнюю установку. Резервные копии: $($state.Stage)" `
        "component-installation-rollback: Could not restore the previous installation. Backups: $($state.Stage)")
    }
  }
  $claim = Join-Path $state.Root ".eska-components.claim"
  if ((Test-Path -LiteralPath $claim -PathType Leaf) -and ([IO.File]::ReadAllText($claim) -eq $state.Token)) {
    Remove-Item -LiteralPath $claim -Force -ErrorAction Stop
  }
  Remove-Item -LiteralPath $state.Stage -Recurse -Force -ErrorAction Stop
  $script:EskaFullInstall = $null
}

function Start-EskaFullInstall($BinDirectory, $Binaries) {
  $bin = [IO.Path]::GetFullPath($BinDirectory).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
  if ((Split-Path -Path $bin -Leaf) -ne "bin") { return }
  $root = Split-Path -Path $bin -Parent
  $components = Join-Path $root "components"
  $names = @("eska", "eska-project", "eska-check", "eska-build", "eska-vcs", "eska-fmt")
  $receipts = @($names | ForEach-Object { Join-Path $components "$_.json" } | Where-Object { Get-EskaItem $_ })
  if ($receipts.Count -eq 0) { return }
  if (-not $install_updater) {
    throw (Get-EskaMessage `
      "component-installation-receipt-required: Для перехода с Explorer на полный комплект запустите установщик без ESKA_DISABLE_UPDATE и ESKA_UNMANAGED_INSTALL." `
      "component-installation-receipt-required: To replace Explorer components with the full bundle, run without ESKA_DISABLE_UPDATE and ESKA_UNMANAGED_INSTALL.")
  }
  $directory = Get-EskaItem $components
  if (-not $directory.PSIsContainer -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw (Get-EskaMessage "component-installation-path: Недопустимый каталог $components" `
      "component-installation-path: Unsafe directory $components")
  }
  $stage = Join-Path $root (".eska-full-install." + [Guid]::NewGuid().ToString("N"))
  $null = New-Item -ItemType Directory -Path $stage -ErrorAction Stop
  $token = @{ schema_version = 1; pid = $PID; nonce = [Guid]::NewGuid().ToString("N") } | ConvertTo-Json -Compress
  $state = @{ Root = $root; Stage = $stage; Token = $token; Ready = $false; Committed = $false;
    Entries = @(); Receipts = $receipts }
  $script:EskaFullInstall = $state
  $claim = Join-Path $root ".eska-components.claim"
  try {
    $stream = [IO.File]::Open($claim, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try {
      $bytes = [Text.Encoding]::UTF8.GetBytes($token)
      $stream.Write($bytes, 0, $bytes.Length)
      $stream.Flush($true)
    } finally { $stream.Dispose() }
  } catch {
    throw (Get-EskaMessage "component-installation-busy: Дождитесь завершения установки Explorer: $root" `
      "component-installation-busy: Wait for the Explorer installation to finish: $root")
  }
  $pending = @(Join-Path $root ".eska-components.recovering")
  $pending += @(Get-ChildItem -LiteralPath $root -Directory -Filter ".eska-component-*" -Force |
    ForEach-Object { Join-Path $_.FullName "journal.json" })
  foreach ($path in $pending) {
    if (Get-EskaItem $path) {
      throw (Get-EskaMessage "component-installation-busy: Сначала восстановите прерванную установку: $path" `
        "component-installation-busy: Recover the interrupted installation first: $path")
    }
  }
  # Include components whose previous writer finished before this claim was acquired.
  $receipts = @($names | ForEach-Object { Join-Path $components "$_.json" } | Where-Object { Get-EskaItem $_ })
  $state.Receipts = $receipts
  $null = New-Item -ItemType Directory -Path (Join-Path $stage "bin"), (Join-Path $stage "components") -ErrorAction Stop
  foreach ($binary in $Binaries) {
    $name = Split-Path -Path $binary -Leaf
    $state.Entries += @{ Path = Join-Path $bin $name; Backup = Join-Path $stage "bin/$name" }
  }
  foreach ($path in $receipts) {
    $state.Entries += @{ Path = $path; Backup = Join-Path $stage ("components/" + (Split-Path $path -Leaf)) }
  }
  $state.Entries += @{ Path = Join-Path $receipt_home "eska-receipt.json"; Backup = Join-Path $stage "installer-receipt.json" }
  foreach ($entry in $state.Entries) {
    $item = Get-EskaItem $entry.Path
    if ($null -eq $item) { continue }
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
      throw (Get-EskaMessage "component-installation-path: Недопустимый файл $($entry.Path)" `
        "component-installation-path: Unsafe file $($entry.Path)")
    }
    Copy-Item -LiteralPath $entry.Path -Destination $entry.Backup -ErrorAction Stop
  }
  $state.Ready = $true
  Write-Information (Get-EskaMessage "Обновление полного комплекта ESKA до $app_version, включая компоненты Explorer." `
    "Updating the complete ESKA bundle to $app_version, including Explorer components.")
}

function Complete-EskaFullInstall {
  $state = $script:EskaFullInstall
  if ($null -eq $state) { return }
  foreach ($path in $state.Receipts) {
    Remove-Item -LiteralPath $path -ErrorAction Stop
  }
  $state.Committed = $true
}
