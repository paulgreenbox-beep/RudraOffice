param(
    [Parameter(Mandatory)]
    [string] $Png
)

# Expected placement for fixtures/public/slides/supported-roundtrip.pptx,
# slide 2, fit at 100%, on the 1240x780 default hidden MSS test window.
# OOXML: 12192000x6858000 slide, image (914400, 1800000, 1219200, 1219200).
# The existing stage at (201, 214) is 1005x565; sample inside the image,
# excluding its antialiased edge. Non-white near the center alone is not a pass.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing.Common
$path = (Resolve-Path -LiteralPath $Png).ProviderPath
$bitmap = [Drawing.Bitmap]::new($path)
try {
    if ($bitmap.Width -ne 1240 -or $bitmap.Height -ne 780) {
        throw 'Capture requires the fixture at the 1240x780 MSS profile size.'
    }
    function Measure-Pixels([int] $Left, [int] $Top, [int] $Width, [int] $Height) {
        $colored = 0
        for ($row = $Top; $row -lt $Top + $Height; $row++) {
            for ($col = $Left; $col -lt $Left + $Width; $col++) {
                $pixel = $bitmap.GetPixel($col, $row)
                if ($pixel.R -lt 245 -or $pixel.G -lt 245 -or $pixel.B -lt 245) {
                    $colored++
                }
            }
        }
        $colored
    }
    $tiles = @(foreach ($left in @(285, 337)) {
        foreach ($top in @(372, 424)) { Measure-Pixels $left $top 30 30 }
    })
    $total = Measure-Pixels 284 369 87 87
    $outside = Measure-Pixels 387 370 20 87
    # These are the decoded RGB corners of the fixture PNG. Confirm the
    # actual picture, not a non-white placeholder of the expected size.
    $corners = @(
        @(290, 375, 230, 55, 45),
        @(364, 375, 45, 180, 95),
        @(290, 450, 55, 105, 220),
        @(364, 450, 240, 200, 40)
    )
    $cornerErrors = @(foreach ($corner in $corners) {
        $pixel = $bitmap.GetPixel($corner[0], $corner[1])
        [Math]::Abs([int]$pixel.R - $corner[2]) + [Math]::Abs([int]$pixel.G - $corner[3]) +
            [Math]::Abs([int]$pixel.B - $corner[4])
    })
    $pass = ($total -ge 6600) -and (@($tiles | Where-Object { $_ -lt 750 }).Count -eq 0) -and
        ($outside -le 5) -and (@($cornerErrors | Where-Object { $_ -gt 85 }).Count -eq 0)
    [ordered]@{
        status = $(if ($pass) { 'PASS' } else { 'FAIL_PRODUCT' })
        png_sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
        interior_colored = $total
        four_tiles_colored = $tiles
        outside_colored = $outside
        fixture_corner_color_errors = $cornerErrors
    } | ConvertTo-Json -Compress
    if (-not $pass) { exit 1 }
} finally {
    $bitmap.Dispose()
}
