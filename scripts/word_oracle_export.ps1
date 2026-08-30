param(
    [Parameter(Mandatory = $true)]
    [string]$JobPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ExpectedFontBytes = 825628
$ExpectedFontSha256 = "f5f552c8c5edb61fe6efb824baf4d4de47b1a8689ab4925ff43f7bd6a4ebece5"
$ExpectedFontName = "NotoSans-Regular.ttf"
$ExpectedPostScriptName = "NotoSans-Regular"
$ExpectedDocumentCount = 48

function Assert-ExactProperties {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string[]]$Expected,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if ($null -eq $Value) {
        throw "$Label must be an object"
    }
    $actual = @($Value.PSObject.Properties.Name | Sort-Object)
    $wanted = @($Expected | Sort-Object)
    if (($actual -join "`n") -cne ($wanted -join "`n")) {
        throw "$Label properties differ"
    }
}

function Get-RegularFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][long]$MaximumBytes,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "$Label must be a regular non-reparse file"
    }
    if ($item.Length -le 0 -or $item.Length -gt $MaximumBytes) {
        throw "$Label size is outside the contract"
    }
    return $item
}

function Get-LowerSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-CanonicalCaseId {
    param([Parameter(Mandatory = $true)][string]$CaseId)
    if ($CaseId -cnotmatch "^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$") {
        throw "case ID is not canonical"
    }
}

function Assert-AbsolutePath {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if (-not [IO.Path]::IsPathRooted($Path)) {
        throw "$Label must be absolute"
    }
}

function Get-ProducerIdentity {
    param([Parameter(Mandatory = $true)]$Runtime)
    $keys = @(
        "application",
        "version",
        "build",
        "executable_sha256",
        "os_version",
        "os_build",
        "machine",
        "powershell_version"
    )
    $rows = foreach ($key in $keys) {
        "$key=$($Runtime.$key)"
    }
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($rows -join "`n"))
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace("-", "").ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

$jobItem = Get-RegularFile -Path $JobPath -MaximumBytes (8 * 1024 * 1024) -Label "job"
$jobText = [IO.File]::ReadAllText($jobItem.FullName, [Text.UTF8Encoding]::new($false, $true))
$job = $jobText | ConvertFrom-Json
Assert-ExactProperties $job @(
    "schema", "run_id", "output_directory", "metadata_path", "font", "export", "documents"
) "job"
if ($job.schema -cne "rwml.word-export-job.v1") {
    throw "job schema is invalid"
}
if ($job.run_id -cnotmatch "^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$") {
    throw "run ID is not canonical"
}

Assert-ExactProperties $job.font @(
    "path", "family", "postscript_name", "bytes", "sha256"
) "job font"
if (
    $job.font.family -cne "Noto Sans" -or
    $job.font.postscript_name -cne $ExpectedPostScriptName -or
    [long]$job.font.bytes -ne $ExpectedFontBytes -or
    $job.font.sha256 -cne $ExpectedFontSha256
) {
    throw "job font does not match the fixed lock"
}
Assert-AbsolutePath $job.font.path "font path"
$fontItem = Get-RegularFile -Path $job.font.path -MaximumBytes (16 * 1024 * 1024) -Label "font"
if (
    $fontItem.Name -cne $ExpectedFontName -or
    $fontItem.Length -ne $ExpectedFontBytes -or
    (Get-LowerSha256 $fontItem.FullName) -cne $ExpectedFontSha256
) {
    throw "installed font does not match the fixed lock"
}
$allowedFontDirectories = @(
    [IO.Path]::GetFullPath((Join-Path $env:WINDIR "Fonts")),
    [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA "Microsoft\Windows\Fonts"))
)
if ($allowedFontDirectories -notcontains $fontItem.Directory.FullName) {
    throw "locked font is not installed in a Windows font directory"
}

Assert-ExactProperties $job.export @(
    "bitmap_missing_fonts", "bookmarks", "document_structure_tags", "format",
    "include_document_properties", "item", "keep_irm", "optimize_for", "pdfa", "range"
) "export options"
if (
    [bool]$job.export.bitmap_missing_fonts -ne $false -or
    $job.export.bookmarks -cne "none" -or
    [bool]$job.export.document_structure_tags -ne $true -or
    $job.export.format -cne "pdf" -or
    [bool]$job.export.include_document_properties -ne $true -or
    $job.export.item -cne "document-content" -or
    [bool]$job.export.keep_irm -ne $true -or
    $job.export.optimize_for -cne "print" -or
    [bool]$job.export.pdfa -ne $false -or
    $job.export.range -cne "all-document"
) {
    throw "Word export options differ from the fixed contract"
}

Assert-AbsolutePath $job.output_directory "output directory"
Assert-AbsolutePath $job.metadata_path "metadata path"
$outputDirectory = [IO.Path]::GetFullPath($job.output_directory)
$metadataPath = [IO.Path]::GetFullPath($job.metadata_path)
if (Test-Path -LiteralPath $outputDirectory) {
    throw "output directory must be fresh"
}
if (Test-Path -LiteralPath $metadataPath) {
    throw "metadata path must be fresh"
}
if ([IO.Path]::GetDirectoryName($metadataPath) -cne [IO.Path]::GetDirectoryName($outputDirectory)) {
    throw "metadata and PDF directories must share the run directory"
}

$documents = @($job.documents)
if ($documents.Count -ne $ExpectedDocumentCount) {
    throw "job must contain exactly 48 documents"
}
$seen = @{}
$previousCaseId = ""
foreach ($row in $documents) {
    Assert-ExactProperties $row @(
        "case_id", "input", "output", "input_bytes", "input_sha256"
    ) "job document"
    Assert-CanonicalCaseId $row.case_id
    if ($seen.ContainsKey($row.case_id) -or ($previousCaseId -and $row.case_id -cle $previousCaseId)) {
        throw "job documents must have unique, sorted case IDs"
    }
    $seen[$row.case_id] = $true
    $previousCaseId = $row.case_id
    Assert-AbsolutePath $row.input "document input"
    Assert-AbsolutePath $row.output "PDF output"
    $inputItem = Get-RegularFile -Path $row.input -MaximumBytes (256 * 1024 * 1024) -Label "document input"
    if (
        $inputItem.Length -ne [long]$row.input_bytes -or
        (Get-LowerSha256 $inputItem.FullName) -cne $row.input_sha256
    ) {
        throw "document input identity mismatch for $($row.case_id)"
    }
    $expectedOutput = [IO.Path]::GetFullPath((Join-Path $outputDirectory "$($row.case_id).pdf"))
    if ([IO.Path]::GetFullPath($row.output) -cne $expectedOutput) {
        throw "PDF output path is not canonical for $($row.case_id)"
    }
}

[IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$word = $null
$document = $null
$metadataRows = [Collections.Generic.List[object]]::new()
try {
    $word = New-Object -ComObject Word.Application
    $word.AutomationSecurity = 3
    $word.DisplayAlerts = 0
    $word.Visible = $false
    $word.Options.ConfirmConversions = $false
    $word.Options.SaveNormalPrompt = $false

    $wordExecutable = [IO.Path]::GetFullPath((Join-Path $word.Path "WINWORD.EXE"))
    $wordExecutableItem = Get-RegularFile -Path $wordExecutable -MaximumBytes (2 * 1024 * 1024 * 1024) -Label "Word executable"
    $osVersion = [Environment]::OSVersion.Version.ToString()
    $osBuild = (Get-ItemProperty -LiteralPath "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion" -Name CurrentBuildNumber).CurrentBuildNumber.ToString()
    $machine = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    $runtime = [ordered]@{
        application = "Microsoft Word"
        version = $word.Version.ToString()
        build = $word.Build.ToString()
        executable_sha256 = Get-LowerSha256 $wordExecutableItem.FullName
        os_version = $osVersion
        os_build = $osBuild
        machine = $machine.ToString()
        powershell_version = $PSVersionTable.PSVersion.ToString()
    }
    $producerIdentity = Get-ProducerIdentity $runtime

    foreach ($row in $documents) {
        $inputPath = [IO.Path]::GetFullPath($row.input)
        $outputPath = [IO.Path]::GetFullPath($row.output)
        try {
            $document = $word.Documents.Open($inputPath, $false, $true, $false)
            $document.ExportAsFixedFormat(
                $outputPath,
                17,
                $false,
                0,
                0,
                1,
                1,
                0,
                $true,
                $true,
                0,
                $true,
                $false,
                $false
            )
        }
        finally {
            if ($null -ne $document) {
                $document.Close(0)
                [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($document)
                $document = $null
            }
        }
        $pdfItem = Get-RegularFile -Path $outputPath -MaximumBytes (64 * 1024 * 1024) -Label "PDF output"
        $metadataRows.Add([ordered]@{
            case_id = $row.case_id
            pdf_bytes = [long]$pdfItem.Length
            pdf_sha256 = Get-LowerSha256 $pdfItem.FullName
        })
    }

    $metadata = [ordered]@{
        schema = "rwml.word-export-metadata.v1"
        run_id = $job.run_id
        producer = [ordered]@{
            name = "microsoft-word"
            mode = "windows-com"
            version = "Microsoft Word $($runtime.version) build $($runtime.build)"
            identity_sha256 = $producerIdentity
            platform = [ordered]@{
                system = "Windows"
                release = "$($runtime.os_version) build $($runtime.os_build)"
                machine = $runtime.machine
            }
        }
        runtime = $runtime
        font = [ordered]@{
            family = "Noto Sans"
            postscript_name = $ExpectedPostScriptName
            bytes = $ExpectedFontBytes
            sha256 = $ExpectedFontSha256
            installed_font_directory = $true
        }
        export = [ordered]@{
            bitmap_missing_fonts = $false
            bookmarks = "none"
            document_structure_tags = $true
            format = "pdf"
            include_document_properties = $true
            item = "document-content"
            keep_irm = $true
            optimize_for = "print"
            pdfa = $false
            range = "all-document"
        }
        documents = @($metadataRows)
    }
    $metadataJson = $metadata | ConvertTo-Json -Depth 8
    $temporaryMetadata = "$metadataPath.tmp-$([Guid]::NewGuid().ToString('N'))"
    [IO.File]::WriteAllText($temporaryMetadata, $metadataJson + "`n", [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporaryMetadata -Destination $metadataPath
}
finally {
    if ($null -ne $document) {
        try { $document.Close(0) } catch { }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($document)
    }
    if ($null -ne $word) {
        try { $word.Quit(0) } catch { }
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($word)
    }
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}
