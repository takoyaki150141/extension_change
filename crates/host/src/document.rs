use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, Context as _, Result};
use calamine::{open_workbook_auto, Data, Reader};
use pulldown_cmark::{html, Options, Parser};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub fn excel_to_csv(input: &Path, output: &Path) -> Result<()> {
    let mut workbook = open_workbook_auto(input)
        .with_context(|| format!("cannot open workbook: {}", input.display()))?;
    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| anyhow!("workbook has no sheets"))??;

    let file = fs::File::create(output)?;
    let mut writer = csv::WriterBuilder::new()
        .flexible(true)
        .from_writer(file);
    for row in range.rows() {
        let record: Vec<String> = row.iter().map(cell_text).collect();
        writer.write_record(record)?;
    }
    writer.flush()?;
    Ok(())
}

fn cell_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::String(value) => value.clone(),
        Data::Float(value) => value.to_string(),
        Data::Int(value) => value.to_string(),
        Data::Bool(value) => value.to_string(),
        Data::DateTime(value) => value.to_string(),
        Data::DateTimeIso(value) => value.clone(),
        Data::DurationIso(value) => value.clone(),
        Data::Error(value) => format!("#ERROR:{value:?}"),
    }
}

pub fn csv_to_excel(input: &Path, output: &Path) -> Result<()> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(input)?;
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let worksheet = workbook.add_worksheet();

    for (row, record) in reader.records().enumerate() {
        let record = record?;
        for (column, field) in record.iter().enumerate() {
            worksheet.write_string(row as u32, column as u16, field)?;
        }
    }

    workbook.save(output)?;
    Ok(())
}

pub fn markdown_to_html(input: &Path, output: &Path) -> Result<()> {
    let markdown = fs::read_to_string(input)?;
    let body = render_markdown(&markdown);
    let title = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Document");
    let html = format!(
        "<!doctype html>\n<html lang=\"ja\"><head><meta charset=\"utf-8\"><title>{}</title></head><body>{}</body></html>\n",
        escape_html(title),
        body
    );
    fs::write(output, html)?;
    Ok(())
}

fn render_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(markdown, options);
    let mut body = String::new();
    html::push_html(&mut body, parser);
    body
}

pub fn markdown_to_epub(input: &Path, output: &Path) -> Result<()> {
    let markdown = fs::read_to_string(input)?;
    let body = render_markdown(&markdown);
    let title = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Document");
    let title = escape_html(title);

    let file = fs::File::create(output)?;
    let mut zip = ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored)?;
    zip.write_all(b"application/epub+zip")?;

    zip.start_file("META-INF/container.xml", deflated)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )?;

    zip.start_file("OEBPS/content.xhtml", deflated)?;
    let xhtml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"ja\"><head><title>{title}</title></head><body>{body}</body></html>"
    );
    zip.write_all(xhtml.as_bytes())?;

    zip.start_file("OEBPS/nav.xhtml", deflated)?;
    zip.write_all(
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>{title}</title></head><body><nav epub:type=\"toc\"><h1>{title}</h1><ol><li><a href=\"content.xhtml\">{title}</a></li></ol></nav></body></html>"
        )
        .as_bytes(),
    )?;

    zip.start_file("OEBPS/content.opf", deflated)?;
    let opf = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"bookid\">urn:uuid:extension-change</dc:identifier><dc:title>{title}</dc:title><dc:language>ja</dc:language><meta property='dcterms:modified'>2026-01-01T00:00:00Z</meta></metadata><manifest><item id=\"content\" href=\"content.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/></manifest><spine><itemref idref=\"content\"/></spine></package>"
    );
    zip.write_all(opf.as_bytes())?;

    zip.finish()?;
    Ok(())
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
