/// Minimal .docx writer — builds OOXML ZIP directly.
/// Supports: headings, OMML math paragraphs, PNG/SVG images.

use std::io::Write;
use zip::{ZipWriter, write::SimpleFileOptions};

pub struct DocxBuilder {
    paragraphs: Vec<DocxPara>,
    images:     Vec<ImageEntry>,
}

struct ImageEntry {
    id:     String,
    bytes:  Vec<u8>,
    is_svg: bool,
    w_emu:  u32,
    h_emu:  u32,
}

enum DocxPara {
    Heading(String, u8),
    Text(String),
    Math(String),
    Image(String, u32, u32),  // rel_id, w_emu, h_emu
}

impl DocxBuilder {
    pub fn new() -> Self {
        Self { paragraphs: Vec::new(), images: Vec::new() }
    }

    pub fn heading(mut self, text: &str, level: u8) -> Self {
        self.paragraphs.push(DocxPara::Heading(text.to_string(), level));
        self
    }

    pub fn text(mut self, text: &str) -> Self {
        if !text.is_empty() {
            self.paragraphs.push(DocxPara::Text(text.to_string()));
        }
        self
    }

    pub fn math(mut self, omml: String) -> Self {
        self.paragraphs.push(DocxPara::Math(omml));
        self
    }

    /// Embed PNG — width/height in logical pixels, scaled to max 450pt in document
    pub fn image(mut self, png: Vec<u8>, width_px: u32, height_px: u32) -> Self {
        let id = format!("rId{}", self.images.len() + 10);
        let max_emu: u32 = 450 * 12700;
        let w_emu = (width_px * 9525).min(max_emu);
        let h_emu = (height_px as f64 * w_emu as f64 / (width_px * 9525) as f64) as u32;
        self.paragraphs.push(DocxPara::Image(id.clone(), w_emu, h_emu));
        self.images.push(ImageEntry { id, bytes: png, is_svg: false, w_emu, h_emu });
        self
    }

    /// Embed SVG — width/height in SVG user units (will be scaled to max 450pt wide)
    pub fn svg_image(mut self, svg_bytes: Vec<u8>, width_pt: u32, height_pt: u32) -> Self {
        let id = format!("rId{}", self.images.len() + 10);
        // Cap at 450pt wide, scale height proportionally. 1pt = 12700 EMU.
        let max_emu: u32 = 450 * 12700;
        let w_emu = (width_pt * 12700).min(max_emu);
        let h_emu = (height_pt as f64 * w_emu as f64 / (width_pt * 12700) as f64) as u32;
        self.paragraphs.push(DocxPara::Image(id.clone(), w_emu, h_emu));
        self.images.push(ImageEntry { id, bytes: svg_bytes, is_svg: true, w_emu, h_emu });
        self
    }

    pub fn build(self) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip = ZipWriter::new(cursor);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", opts).unwrap();
        zip.write_all(content_types(&self.images).as_bytes()).unwrap();

        zip.start_file("_rels/.rels", opts).unwrap();
        zip.write_all(ROOT_RELS.as_bytes()).unwrap();

        zip.start_file("word/_rels/document.xml.rels", opts).unwrap();
        zip.write_all(document_rels(&self.images).as_bytes()).unwrap();

        zip.start_file("word/document.xml", opts).unwrap();
        zip.write_all(build_document(&self.paragraphs).as_bytes()).unwrap();

        zip.start_file("word/styles.xml", opts).unwrap();
        zip.write_all(STYLES_XML.as_bytes()).unwrap();

        zip.start_file("word/settings.xml", opts).unwrap();
        zip.write_all(SETTINGS_XML.as_bytes()).unwrap();

        for img in &self.images {
            let ext = if img.is_svg { "svg" } else { "png" };
            zip.start_file(format!("word/media/{}.{ext}", img.id), opts).unwrap();
            zip.write_all(&img.bytes).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }
}

// ─── document.xml ────────────────────────────────────────────────────────────

fn build_document(paras: &[DocxPara]) -> String {
    let mut body = String::new();
    let mut img_counter = 0usize;

    for para in paras {
        match para {
            DocxPara::Heading(text, level) => {
                let style = format!("Heading{level}");
                body.push_str(&format!(
                    "<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr>\
                     <w:r><w:t>{}</w:t></w:r></w:p>",
                    xml_esc(text)
                ));
            }
            DocxPara::Text(text) => {
                body.push_str(&format!(
                    "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                    xml_esc(text)
                ));
            }
            DocxPara::Math(omml) => {
                body.push_str(&format!(
                    "<w:p><m:oMathPara>\
                     <m:oMathParaPr><m:jc m:val=\"center\"/></m:oMathParaPr>\
                     {omml}\
                     </m:oMathPara></w:p>"
                ));
            }
            DocxPara::Image(rel_id, w_emu, h_emu) => {
                img_counter += 1;
                body.push_str(&inline_image(rel_id, *w_emu, *h_emu, img_counter));
            }
        }
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document \
           xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" \
           xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
           xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
           xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
           xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
           xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
         <w:body>{body}<w:sectPr/></w:body></w:document>"
    )
}

fn inline_image(rel_id: &str, w_emu: u32, h_emu: u32, idx: usize) -> String {
    let draw_id = idx as u32;
    format!(
        "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>\
         <w:r><w:drawing>\
         <wp:inline distT='0' distB='0' distL='0' distR='0'>\
         <wp:extent cx='{w_emu}' cy='{h_emu}'/>\
         <wp:effectExtent l='0' t='0' r='0' b='0'/>\
         <wp:docPr id='{draw_id}' name='Image{draw_id}'/>\
         <wp:cNvGraphicFramePr>\
           <a:graphicFrameLocks xmlns:a='http://schemas.openxmlformats.org/drawingml/2006/main' noChangeAspect='1'/>\
         </wp:cNvGraphicFramePr>\
         <a:graphic xmlns:a='http://schemas.openxmlformats.org/drawingml/2006/main'>\
         <a:graphicData uri='http://schemas.openxmlformats.org/drawingml/2006/picture'>\
         <pic:pic xmlns:pic='http://schemas.openxmlformats.org/drawingml/2006/picture'>\
           <pic:nvPicPr>\
             <pic:cNvPr id='{draw_id}' name='Image{draw_id}'/>\
             <pic:cNvPicPr/>\
           </pic:nvPicPr>\
           <pic:blipFill>\
             <a:blip r:embed='{rel_id}'/>\
             <a:stretch><a:fillRect/></a:stretch>\
           </pic:blipFill>\
           <pic:spPr>\
             <a:xfrm><a:off x='0' y='0'/><a:ext cx='{w_emu}' cy='{h_emu}'/></a:xfrm>\
             <a:prstGeom prst='rect'><a:avLst/></a:prstGeom>\
           </pic:spPr>\
         </pic:pic>\
         </a:graphicData></a:graphic>\
         </wp:inline></w:drawing></w:r></w:p>"
    )
}

// ─── XML boilerplate ─────────────────────────────────────────────────────────

fn content_types(images: &[ImageEntry]) -> String {
    let img_overrides: String = images.iter().map(|img| {
        let ext = if img.is_svg { "svg" } else { "png" };
        let ct  = if img.is_svg { "image/svg+xml" } else { "image/png" };
        format!("<Override PartName='/word/media/{}.{ext}' ContentType='{ct}'/>", img.id)
    }).collect();

    format!(
        "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\
         <Types xmlns='http://schemas.openxmlformats.org/package/2006/content-types'>\
         <Default Extension='rels' ContentType='application/vnd.openxmlformats-package.relationships+xml'/>\
         <Default Extension='xml'  ContentType='application/xml'/>\
         <Default Extension='png'  ContentType='image/png'/>\
         <Default Extension='svg'  ContentType='image/svg+xml'/>\
         <Override PartName='/word/document.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'/>\
         <Override PartName='/word/styles.xml'   ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml'/>\
         <Override PartName='/word/settings.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml'/>\
         {img_overrides}\
         </Types>"
    )
}

fn document_rels(images: &[ImageEntry]) -> String {
    let img_rels: String = images.iter().map(|img| {
        let ext = if img.is_svg { "svg" } else { "png" };
        format!(
            "<Relationship Id='{}' \
             Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/image' \
             Target='media/{}.{ext}'/>",
            img.id, img.id
        )
    }).collect();

    format!(
        "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\
         <Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>\
         <Relationship Id='rId1' \
           Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles' \
           Target='styles.xml'/>\
         <Relationship Id='rId2' \
           Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings' \
           Target='settings.xml'/>\
         {img_rels}\
         </Relationships>"
    )
}

const ROOT_RELS: &str = "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>
<Relationships xmlns='http://schemas.openxmlformats.org/package/2006/relationships'>
<Relationship Id='rId1' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument' Target='word/document.xml'/>
</Relationships>";

const STYLES_XML: &str = "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>
<w:styles xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'>
<w:style w:type='paragraph' w:styleId='Normal'>
  <w:name w:val='Normal'/>
  <w:pPr><w:spacing w:after='160'/></w:pPr>
  <w:rPr><w:sz w:val='24'/><w:szCs w:val='24'/></w:rPr>
</w:style>
<w:style w:type='paragraph' w:styleId='Heading1'>
  <w:name w:val='heading 1'/><w:basedOn w:val='Normal'/>
  <w:pPr><w:spacing w:before='240' w:after='60'/></w:pPr>
  <w:rPr><w:b/><w:sz w:val='40'/><w:szCs w:val='40'/><w:color w:val='2E74B5'/></w:rPr>
</w:style>
<w:style w:type='paragraph' w:styleId='Heading2'>
  <w:name w:val='heading 2'/><w:basedOn w:val='Normal'/>
  <w:pPr><w:spacing w:before='200' w:after='40'/></w:pPr>
  <w:rPr><w:b/><w:sz w:val='32'/><w:szCs w:val='32'/><w:color w:val='2E74B5'/></w:rPr>
</w:style>
<w:style w:type='paragraph' w:styleId='Heading3'>
  <w:name w:val='heading 3'/><w:basedOn w:val='Normal'/>
  <w:pPr><w:spacing w:before='160' w:after='40'/></w:pPr>
  <w:rPr><w:b/><w:sz w:val='28'/><w:szCs w:val='28'/><w:color w:val='2E74B5'/></w:rPr>
</w:style>
</w:styles>";

const SETTINGS_XML: &str = "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>
<w:settings xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'>
<w:compat><w:compatSetting w:name='compatibilityMode' w:uri='http://schemas.microsoft.com/office/word' w:val='15'/></w:compat>
</w:settings>";

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
