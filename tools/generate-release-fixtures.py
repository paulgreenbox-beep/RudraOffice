"""Generate deterministic DOCX and PDF fixtures for Rudra Office release acceptance.

The script is deliberately narrow: it does not mutate product code, and it
only writes the two named corpus files below the repository root supplied by
``--root``. The remaining Office fixtures are sourced from first-party test
material and tracked through the corpus manifest.
"""

from __future__ import annotations

import argparse
import os
import tempfile
import zipfile
from pathlib import Path

from docx import Document
from docx.enum.text import WD_BREAK
from docx.shared import Mm, Pt
from reportlab.lib.pagesizes import A4, letter, landscape
from reportlab.lib.utils import ImageReader
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfgen import canvas


DOCX_NAME = "writer-layout-a4-multipage.docx"
PDF_NAME = "mixed-pages-rotated.pdf"


def extract_fixture_jpeg(source_docx: Path, output_dir: Path) -> Path:
    with zipfile.ZipFile(source_docx) as archive:
        candidates = [
            entry for entry in archive.namelist() if entry.startswith("word/media/")
        ]
        if not candidates:
            raise RuntimeError(f"no embedded image found in {source_docx}")
        source = candidates[0]
        output = output_dir / Path(source).name
        output.write_bytes(archive.read(source))
        return output


def configure_docx(document: Document) -> None:
    section = document.sections[0]
    section.page_width = Mm(210)
    section.page_height = Mm(297)
    section.top_margin = Mm(18)
    section.bottom_margin = Mm(18)
    section.left_margin = Mm(18)
    section.right_margin = Mm(18)

    normal = document.styles["Normal"]
    normal.font.name = "Arial"
    normal.font.size = Pt(11)

    header = section.header.paragraphs[0]
    header.text = "Rudra Office — controlled acceptance"
    header.runs[0].font.size = Pt(9)
    footer = section.footer.paragraphs[0]
    footer.text = "Viewer-only fixture · page layout and Unicode smoke"
    footer.runs[0].font.size = Pt(9)


def build_docx(output: Path, image: Path) -> None:
    document = Document()
    configure_docx(document)

    title = document.add_heading("Контролируемая приёмка Writer", level=0)
    title.runs[0].font.name = "Arial"
    intro = document.add_paragraph()
    intro.add_run("Кириллица: ").bold = True
    intro.add_run(
        "документ содержит текст, таблицу, встроенное изображение, "
        "верхний и нижний колонтитулы и вторую страницу."
    )

    table = document.add_table(rows=1, cols=3)
    table.style = "Table Grid"
    header_cells = table.rows[0].cells
    for cell, text in zip(header_cells, ("№", "Проверка", "Ожидаемый результат")):
        cell.text = text
        cell.paragraphs[0].runs[0].bold = True
    rows = (
        ("1", "Текст и Unicode", "Русский текст отображается"),
        ("2", "Таблица", "Границы и строки видимы"),
        ("3", "Изображение", "Встроенное изображение не пропадает"),
    )
    for number, check, expected in rows:
        cells = table.add_row().cells
        cells[0].text = number
        cells[1].text = check
        cells[2].text = expected

    document.add_paragraph("Встроенное изображение ниже является частью fixture.")
    document.add_picture(str(image), width=Mm(42))
    document.add_paragraph().add_run().add_break(WD_BREAK.PAGE)

    document.add_heading("Вторая страница", level=1)
    for index in range(1, 7):
        document.add_paragraph(
            f"Строка {index}. Эта страница подтверждает постраничное отображение "
            "и прокрутку без изменения исходного файла."
        )
    document.save(output)


def find_unicode_font() -> Path:
    windows = Path(os.environ.get("WINDIR", r"C:\\Windows")) / "Fonts"
    for name in ("arial.ttf", "calibri.ttf", "segoeui.ttf"):
        candidate = windows / name
        if candidate.exists():
            return candidate
    raise RuntimeError("a Windows TrueType font is required to create the Unicode PDF fixture")


def build_pdf(output: Path, image: Path) -> None:
    font_path = find_unicode_font()
    font_name = "RudraCorpusUnicode"
    pdfmetrics.registerFont(TTFont(font_name, str(font_path)))
    document = canvas.Canvas(str(output), pagesize=A4)
    document.setTitle("Rudra Office controlled acceptance PDF")
    document.setAuthor("Rudra Office")

    document.setFont(font_name, 16)
    document.drawString(54, 790, "Контролируемая приёмка PDF — страница 1")
    document.setFont(font_name, 11)
    document.drawString(54, 760, "A4 portrait · Unicode · встроенное изображение")
    document.drawImage(ImageReader(str(image)), 54, 590, width=130, height=105, preserveAspectRatio=True)
    document.showPage()

    document.setPageSize(landscape(A4))
    document.setPageRotation(90)
    document.setFont(font_name, 16)
    document.drawString(54, 540, "Страница 2: A4 landscape, Rotate 90")
    document.setFont(font_name, 11)
    document.drawString(54, 510, "Проверка mixed page sizes и rotated page")
    document.showPage()

    document.setPageRotation(0)
    document.setPageSize(letter)
    document.setFont(font_name, 16)
    document.drawString(54, 740, "Page 3: Letter portrait")
    document.setFont(font_name, 11)
    document.drawString(54, 710, "Final PDF page for navigation and visible canvas smoke.")
    document.save()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    corpus = root / "fixtures" / "release-acceptance"
    docx_output = corpus / "docx" / DOCX_NAME
    pdf_output = corpus / "pdf" / PDF_NAME
    source_docx = root / "fixtures" / "docx" / "writer-visual-fidelity-jpeg-fixture.docx"
    if not source_docx.is_file():
        raise FileNotFoundError(source_docx)
    docx_output.parent.mkdir(parents=True, exist_ok=True)
    pdf_output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="rudra-office-release-fixtures-") as directory:
        image = extract_fixture_jpeg(source_docx, Path(directory))
        build_docx(docx_output, image)
        build_pdf(pdf_output, image)
    print(docx_output)
    print(pdf_output)


if __name__ == "__main__":
    main()
