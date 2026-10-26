"""Extract one embedded font and one text run; never modify the source PDF."""
import argparse
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

PHRASE = (
    "023C0267026D026C026A025F026702670262025F0003026B025F026C02620003"
    "025C0268025E0268026B0267025A025B0260025F026702620279000302620003"
    "0264025A0267025A026502620261025A027002620262"
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("fresh_output", type=Path)
    args = parser.parse_args()
    reader = PdfReader(args.source)
    font = reader.pages[0]["/Resources"]["/Font"]["/C2_2"].get_object()
    if font["/Subtype"] != "/Type0" or font["/Encoding"] != "/Identity-H":
        raise ValueError("source font no longer matches the diagnosed Type0 Identity-H trigger")
    args.fresh_output.mkdir(parents=True, exist_ok=False)
    for name, spacing in (("cid-unicode-space-tw", "40.229"), ("cid-unicode-space-control", "0")):
        writer = PdfWriter()
        page = writer.add_blank_page(width=595, height=120)
        page[NameObject("/Resources")] = DictionaryObject({
            NameObject("/Font"): DictionaryObject({NameObject("/F0"): font.clone(writer)})
        })
        stream = DecodedStreamObject()
        stream.set_data(f"BT /F0 20.041 Tf {spacing} Tw 30 70 Td <{PHRASE}> Tj ET\n".encode("ascii"))
        page[NameObject("/Contents")] = writer._add_object(stream)
        output = args.fresh_output / f"{name}.pdf"
        with output.open("xb") as target:
            writer.write(target)
        print(f"fixture={output} bytes={output.stat().st_size} pages=1 word_spacing={spacing}")


if __name__ == "__main__":
    main()
