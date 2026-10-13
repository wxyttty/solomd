$ErrorActionPreference = 'Stop'

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  fileType       = 'msi'
  url64bit       = 'https://github.com/zhitongblog/solomd/releases/download/v{{VERSION}}/SoloMD_{{VERSION}}_x64_en-US.msi'
  checksum64     = '{{SHA64}}'
  checksumType64 = 'sha256'
  softwareName   = 'SoloMD*'
  silentArgs     = "/qn /norestart /l*v `"$($env:TEMP)\$($env:ChocolateyPackageName).$($env:ChocolateyPackageVersion).MsiInstall.log`""
  validExitCodes = @(0, 3010, 1641)
}

Install-ChocolateyPackage @packageArgs
