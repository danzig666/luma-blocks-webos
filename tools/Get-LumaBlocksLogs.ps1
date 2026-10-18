<#
.SYNOPSIS
  Copy Luma Blocks logs and gesture traces from the TV to a local folder.
.EXAMPLE
  .\Get-LumaBlocksLogs.ps1 -TvHost 192.168.1.50                    # rooted (root over SSH)
.EXAMPLE
  .\Get-LumaBlocksLogs.ps1 -TvHost 192.168.1.50 -DevMode -Key $HOME\.ssh\tv_webos
#>
param(
  [Parameter(Mandatory = $true)][string]$TvHost,
  [switch]$DevMode,
  [string]$Key,
  [switch]$Release,
  [string]$Out = ".\luma-blocks-logs"
)
$ErrorActionPreference = 'Stop'
$AppId = if ($Release) { 'io.github.danzig666.lumablocks' } else { 'io.github.danzig666.lumablocks.debug' }
$Dir = "/media/developer/apps/usr/palm/applications/$AppId/data"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$sshArgs = @()
if ($Key) { $sshArgs += @('-i', $Key) }
if ($DevMode) { $user = 'prisoner'; $sshArgs += @('-P', '9922') } else { $user = 'root' }
scp @sshArgs "${user}@${TvHost}:$Dir/logs/*" $Out
scp @sshArgs "${user}@${TvHost}:$Dir/traces/*" $Out 2>$null
Get-ChildItem $Out
