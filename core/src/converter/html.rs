// TODO(ntbbloodbath): move this converter to a separate rust library called norg-converter
//
// NOTE: the current carryover tags management is the worst boilerplate code I've ever written.
// Refactor later to abstract it even further and make the code cleaner.
//
// BUG: currently, strong carryover tags AST is missing a lot of things in the rust-norg parser
// so we are going to omit them for now until it's fixed.

use std::collections::VecDeque;
use std::sync::OnceLock;

use html_escape::encode_text_minimal_to_string;
use regex::Regex;
use rust_norg::{
    parse_tree, CarryoverTag, DelimitingModifier, LinkTarget, NestableDetachedModifier, NorgAST,
    NorgASTFlat, ParagraphSegment, ParagraphSegmentToken,
};
use tracing::{error, info, warn};

/// CarryOver
#[derive(Clone, Debug)]
struct CarryOverTag {
    name: Vec<String>,
    parameters: Vec<String>,
}

/// ToC entries
#[derive(Clone, Debug)]
pub struct TocEntry {
    level: u16,
    title: String,
    id: String,
}

fn inline_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"-?<.*>").expect("valid regex"))
}

/// Converts paragraph segment tokens to a String
fn paragraph_tokens_to_string(tokens: &[ParagraphSegmentToken]) -> String {
    let mut s = String::new();
    encode_text_minimal_to_string(
        tokens
            .iter()
            .map(|token| match token {
                ParagraphSegmentToken::Text(txt) => txt.clone(),
                ParagraphSegmentToken::Whitespace => String::from(" "),
                ParagraphSegmentToken::Special(c) | ParagraphSegmentToken::Escape(c) => {
                    String::from(*c)
                }
            })
            .collect::<Vec<String>>()
            .join(""),
        &mut s,
    );
    s
}

/// Converts a ParagraphSegment into a String
fn paragraph_to_string(
    segment: &[ParagraphSegment],
    _strong_carry: &[CarryOverTag],
    weak_carry: &mut VecDeque<CarryOverTag>,
    root_url: &str,
) -> String {
    let mut paragraph = String::new();
    segment.iter().for_each(|node| match node {
        ParagraphSegment::Token(t) => match t {
            ParagraphSegmentToken::Text(s) => paragraph.push_str(s),
            ParagraphSegmentToken::Whitespace => paragraph.push(' '),
            ParagraphSegmentToken::Special(c) | ParagraphSegmentToken::Escape(c) => {
                paragraph.push(*c)
            }
        },
        ParagraphSegment::AttachedModifier {
            modifier_type,
            content,
        } => {
            let inner = paragraph_to_string(content, _strong_carry, weak_carry, root_url);
            match modifier_type {
                '*' => { paragraph.push_str("<strong>"); paragraph.push_str(&inner); paragraph.push_str("</strong>"); }
                '/' => { paragraph.push_str("<em>"); paragraph.push_str(&inner); paragraph.push_str("</em>"); }
                '_' => { paragraph.push_str("<u>"); paragraph.push_str(&inner); paragraph.push_str("</u>"); }
                '-' => { paragraph.push_str("<s>"); paragraph.push_str(&inner); paragraph.push_str("</s>"); }
                '^' => { paragraph.push_str("<sup>"); paragraph.push_str(&inner); paragraph.push_str("</sup>"); }
                ',' => { paragraph.push_str("<sub>"); paragraph.push_str(&inner); paragraph.push_str("</sub>"); }
                '!' => { paragraph.push_str("<span class='spoiler'>"); paragraph.push_str(&inner); paragraph.push_str("</span>"); }
                '$' => { paragraph.push_str("<code>"); paragraph.push_str(&inner); paragraph.push_str("</code>"); }
                '%' => {}           // ignore comments
                _ => {
                    info!(
                        "[converter] ParagraphSegment::AttachedModifier: {} {:#?}",
                        modifier_type, content
                    );
                    todo!()
                }
            }
        }
        ParagraphSegment::InlineVerbatim(content) => {
            paragraph.push_str("<code>");
            paragraph.push_str(&paragraph_tokens_to_string(content));
            paragraph.push_str("</code>");
        }
        // ParagraphSegment::AttachedModifierOpener(_) => todo!(),
        // ParagraphSegment::AttachedModifierOpenerFail(_) => todo!(),
        // ParagraphSegment::AttachedModifierCloserCandidate(_) => todo!(),
        // ParagraphSegment::AttachedModifierCloser(_) => todo!(),
        // ParagraphSegment::AttachedModifierCandidate { modifier_type, content, closer } => todo!(),
        ParagraphSegment::Link {
            filepath,
            targets,
            description,
        } => {
            let mut link_name = String::new();
            paragraph.push_str("<a ");

            // link to local paths (':/about:' -> '/about')
            if let Some(path) = filepath {
                if description.is_none() {
                    link_name = path.to_string();
                }
                paragraph.push_str("href=\"");
                paragraph.push_str(root_url);
                paragraph.push_str(path);
                paragraph.push('"');
            }

            // link to anything else
            if !targets.is_empty() {
                match &targets[0] {
                    // link to external URLs
                    LinkTarget::Url(path) | LinkTarget::Path(path) => {
                        if description.is_none() {
                            link_name = path.to_string();
                        }
                        paragraph.push_str("href=\"");
                        paragraph.push_str(path);
                        paragraph.push('"');
                    }
                    LinkTarget::Heading { level: _, title } => {
                        let title_str = paragraph_to_string(title, _strong_carry, weak_carry, root_url);

                        if description.is_none() {
                            link_name = title_str.clone();
                        }
                        paragraph.push_str("href=\"#");
                        paragraph.push_str(&title_str.replace(" ", "-"));
                        paragraph.push('"');
                    }
                    // Missing: Footnote, Definition, Wiki, Generic, Timestamp, Extendable
                    _ => {
                        info!("ParagraphSegment::Link: {:#?}", &node);
                        todo!()
                    }
                }
            }
            if !weak_carry.is_empty() {
                let tags: Vec<_> = weak_carry.drain(..).collect();
                for weak_carryover in tags {
                    paragraph.push(' ');
                    paragraph.push_str(&weak_carryover_attribute(weak_carryover));
                }
            }
            if let Some(desc) = description {
                paragraph.push('>');
                paragraph.push_str(&paragraph_to_string(
                    desc,
                    _strong_carry,
                    weak_carry,
                    root_url
                ));
                paragraph.push_str("</a>");
            } else if link_name.is_empty() {
                paragraph.push_str("></a>");
                warn!("Generated link with no description, make sure all of your Norg links contain a description");
            } else {
                paragraph.push('>');
                paragraph.push_str(&link_name);
                paragraph.push_str("</a>");
            }
        }
        ParagraphSegment::AnchorDefinition { content, target } => {
            paragraph.push_str("<a");
            // XXX: here the ParagraphSegment::Link node only has targets and thus we cannot just recursively use paragraph_to_string
            if let ParagraphSegment::Link {
                filepath: _,
                targets,
                description: _,
            } = *target.clone()
            {
                match &targets[0] {
                    // link to external URLs
                    LinkTarget::Url(path) | LinkTarget::Path(path) => {
                        let href_path = if path.starts_with('/') {
                            format!("{}{}", root_url, path)
                        } else {
                            path.clone()
                        };
                        paragraph.push_str(" href=\"");
                        paragraph.push_str(&href_path);
                        paragraph.push('"');
                    }
                    LinkTarget::Heading { level: _, title } => {
                        // Regex to remove possible links from heading title ids during href
                        let re = inline_re();
                        paragraph.push_str(" href=\"#");
                        paragraph.push_str(&re.replace(
                            &paragraph_to_string(title, _strong_carry, weak_carry, root_url)
                                .replace(" ", "-"),
                            ""
                        ));
                        paragraph.push('"');
                    }
                    // Missing: Footnote, Definition, Wiki, Generic, Timestamp, Extendable
                    _ => {
                        info!("ParagraphSegment::Link: {:#?}", &node);
                        todo!()
                    }
                }
            }
            if !weak_carry.is_empty() {
                let tags: Vec<_> = weak_carry.drain(..).collect();
                for weak_carryover in tags {
                    paragraph.push(' ');
                    paragraph.push_str(&weak_carryover_attribute(weak_carryover));
                }
            }
            paragraph.push('>');
            paragraph.push_str(&paragraph_to_string(&content.clone(), _strong_carry, weak_carry, root_url));
            paragraph.push_str("</a>");
        }
        // ParagraphSegment::Anchor { content, description } => todo!(),
        // ParagraphSegment::InlineLinkTarget(_) => todo!(),
        _ => {
            info!("[converter] ParagraphSegment: {:#?}", node);
            todo!()
        }
    });

    paragraph
}

/// Get the correct tag for an HTML list depending on the type (ordered or unordered) and if it should be opening or closing
fn get_list_tag(mod_type: NestableDetachedModifier, is_opening: bool) -> String {
    match mod_type {
        NestableDetachedModifier::OrderedList => {
            if is_opening {
                String::from("<ol>")
            } else {
                String::from("</ol>")
            }
        }
        NestableDetachedModifier::UnorderedList => {
            if is_opening {
                String::from("<ul>")
            } else {
                String::from("</ul>")
            }
        }
        // NOTE: we do not pass this function to quotes and I think it is impossible to reach a quote with it so this is safe
        _ => unreachable!(),
    }
}

/// Converts a carryover weak tag into a String vector containing an html attribute
fn weak_carryover_attribute(weak_carryover: CarryOverTag) -> String {
    let mut attr = String::new();
    let namespace = &weak_carryover.name[0];
    // XXX: any non-html namespaced weak carryover tag is being ignored right now. Should we keep
    // this behaviour?
    if namespace == "html" {
        if weak_carryover.name.len() < 2 {
            error!("[converter] Carryover tag with namespace 'html' is expected to have an attribute name (e.g. 'html.class')");
        } else if weak_carryover.name.len() >= 3 {
            error!(
                "[converter] Carryover tag with namespace 'html' is expected to have only one attribute name (e.g. 'html.class'), '{}' provided",
                weak_carryover.name.join(".")
            )
        } else {
            let attr_name = weak_carryover.name[1].as_str();
            let values_sep = if attr_name == "style" { ";" } else { " " };

            attr.push_str(attr_name);
            attr.push_str("=\"");
            attr.push_str(&weak_carryover.parameters.join(values_sep));
            attr.push('"');
        }
    }
    attr
}

trait NorgToHtml {
    fn to_html(
        &self,
        strong_carry: &[CarryOverTag],
        weak_carry: VecDeque<CarryOverTag>,
        root_url: &str,
        toc: &mut Vec<TocEntry>,
    ) -> String;
}

impl NorgToHtml for NorgAST {
    // TODO: finish VerbatimRangedTag support, add support for strong carryover tags, footnotes (they are tricky in HTML), anything else that I'm missing
    fn to_html(
        &self,
        strong_carry: &[CarryOverTag],
        mut weak_carry: VecDeque<CarryOverTag>,
        root_url: &str,
        toc: &mut Vec<TocEntry>,
    ) -> String {
        match self {
            NorgAST::Paragraph(s) => {
                let mut paragraph = String::from("<p");
                if !weak_carry.is_empty() {
                    let tags: Vec<_> = weak_carry.drain(..).collect();
                    for weak_carryover in tags {
                        paragraph.push(' ');
                        paragraph.push_str(&weak_carryover_attribute(weak_carryover));
                    }
                }
                paragraph.push('>');
                paragraph.push_str(&paragraph_to_string(s, strong_carry, &mut weak_carry, root_url));
                paragraph.push_str("</p>");
                paragraph
            }
            NorgAST::Heading {
                level,
                title,
                content,
                ..
            } => {
                let mut section = String::new();
                // HACK: we are passing empty carryover vectors here because otherwise
                // the HTML carryovers meant for the heading are used for its internal content instead
                let strong: &[CarryOverTag] = &[];
                let mut weak = VecDeque::<CarryOverTag>::new();
                let heading_title = paragraph_to_string(title, strong, &mut weak, root_url);

                // Regex to remove possible links from heading title ids
                let re = inline_re();
                let heading_id = re.replace(&heading_title.replace(" ", "-"), "").to_string();

                let tag = match level {
                    1..=6 => format!("h{}", level),
                    // XXX: fallback to h6 if the header level is higher than 6
                    _ => String::from("h6"),
                };
                section.push('<');
                section.push_str(&tag);
                section.push_str(" id=\"");
                section.push_str(&heading_id);
                section.push('"');
                if !weak_carry.is_empty() {
                    let tags: Vec<_> = weak_carry.drain(..).collect();
                    for weak_carryover in tags {
                        section.push(' ');
                        section.push_str(&weak_carryover_attribute(weak_carryover));
                    }
                }
                section.push('>');
                section.push_str(&heading_title);
                section.push_str("</");
                section.push_str(&tag);
                section.push('>');
                let entry = TocEntry {
                    level: *level,
                    title: heading_title.clone(),
                    id: heading_id.clone(),
                };
                toc.push(entry);

                section.push_str(&to_html(content, strong_carry, &weak_carry, root_url, toc));

                section
            }
            NorgAST::NestableDetachedModifier {
                modifier_type,
                level: _,
                text,
                content,
                ..
            } => {
                // HACK: 'text' is actually a 'Box<NorgASTFlat>' value. It should be converted into a `ParagraphSegment` later in the rust-norg parser
                let mod_text = if let NorgASTFlat::Paragraph(s) = *text.clone() {
                    let strong: &[CarryOverTag] = &[];
                    let mut weak = VecDeque::<CarryOverTag>::new();
                    // HACK: we are passing empty carryover vectors here because otherwise
                    // the HTML carryovers meant for the lists are used for its internal content instead
                    paragraph_to_string(&s, strong, &mut weak, root_url)
                } else {
                    unreachable!();
                };

                match modifier_type {
                    NestableDetachedModifier::UnorderedList
                    | NestableDetachedModifier::OrderedList => {
                        let mut list = String::from("<li");
                        if !weak_carry.is_empty() {
                            let tags: Vec<_> = weak_carry.drain(..).collect();
                            for weak_carryover in tags {
                                list.push(' ');
                                list.push_str(&weak_carryover_attribute(weak_carryover));
                            }
                        }
                        list.push('>');
                        list.push_str(&mod_text);
                        list.push_str("</li>");
                        if !content.is_empty() {
                            list.push_str(&to_html(content, strong_carry, &weak_carry, root_url, toc));
                        }
                        list
                    }
                    NestableDetachedModifier::Quote => {
                        let mut quote = String::from("<blockquote");
                        if !weak_carry.is_empty() {
                            let tags: Vec<_> = weak_carry.drain(..).collect();
                            for weak_carryover in tags {
                                quote.push(' ');
                                quote.push_str(&weak_carryover_attribute(weak_carryover));
                            }
                        }
                        quote.push('>');
                        quote.push_str(&mod_text);
                        if !content.is_empty() {
                            quote.push_str(&to_html(content, strong_carry, &weak_carry, root_url, toc));
                        }
                        quote.push_str("</blockquote>");
                        quote
                    }
                }
            }
            // VerbatimRangedTag { name: ["code"], parameters: ["lua"], content: "print(\"hello world\")\n" }
            NorgAST::VerbatimRangedTag {
                name,
                parameters,
                content,
            } => {
                let mut verbatim_tag = String::new();
                match name[0].as_str() {
                    "code" => {
                        let mut code_tag = String::from("<pre");
                        if !weak_carry.is_empty() {
                            let tags: Vec<_> = weak_carry.drain(..).collect();
                            for weak_carryover in tags {
                                code_tag.push(' ');
                                code_tag.push_str(&weak_carryover_attribute(weak_carryover));
                            }
                        }
                        // NOTE: Tera completely skips HTML code block contents while rendering our HTML content
                        // because we are forced to use the `safe` filter. This workaround aims to fix those
                        // problems, and (hopefully) also including XML rendering.
                        let content = &tera::escape_html(content);
                        code_tag.push('>');
                        code_tag.push_str("<code");
                        if !parameters.is_empty() {
                            // NOTE: the class `language-foo` is being added by default so the converter can
                            // work out-of-the-box with code highlighting libraries like highlight.js or prismjs
                            code_tag.push_str(" class=\"language-");
                            code_tag.push_str(&parameters[0]);
                            code_tag.push('"');
                        }
                        code_tag.push('>');
                        code_tag.push_str(content);
                        code_tag.push_str("</code></pre>");
                        verbatim_tag = code_tag;
                    }
                    // NOTE: this only works for base64 encoded images, regular images
                    // use the .image infirm tag.
                    "image" => {
                        let mut image_tag = String::from("<img src=\"");
                        image_tag.push_str(content);
                        image_tag.push('"');
                        if !weak_carry.is_empty() {
                            let tags: Vec<_> = weak_carry.drain(..).collect();
                            for weak_carryover in tags {
                                image_tag.push(' ');
                                image_tag.push_str(&weak_carryover_attribute(weak_carryover));
                            }
                        }
                        image_tag.push_str("/>");
                        verbatim_tag = image_tag;
                    }
                    "embed" => {
                        // XXX: only works for embedding HTML code for now
                        if !parameters.is_empty() && parameters[0] == "html" {
                            verbatim_tag = content.to_string()
                        }
                    }
                    // TODO: support other verbatim ranged tags like '@math'
                    _ => {
                        if name[0] != "document" {
                            info!("[converter] VerbatimRangedTag: {:#?}", self);
                            todo!()
                        }
                    }
                }
                verbatim_tag
            }
            // CarryoverTag { tag_type: Attribute, name: ["style"], parameters: ["width:120px;height:120px;"], next_object: VerbatimRangedTag { ... }
            NorgAST::CarryoverTag {
                tag_type,
                name,
                parameters,
                next_object,
            } => match tag_type {
                CarryoverTag::Attribute => {
                    let tag = CarryOverTag {
                        name: name.clone(),
                        parameters: parameters.clone(),
                    };
                    weak_carry.push_back(tag);
                        to_html(
                            &[*next_object.clone()],
                            strong_carry,
                            &weak_carry,
                            root_url,
                            toc,
                        )
                }
                CarryoverTag::Macro => {
                    error!("[converter] Carryover tag macros are unsupported right now");
                    todo!()
                }
            },
            // InfirmTag: InfirmTag { name: ["image"], parameters: ["/assets/norgolith.svg", "Norgolith", "logo"] }
            NorgAST::InfirmTag { name, parameters } => {
                match name[0].as_str() {
                    "image" => {
                        let src_path = if parameters[0].starts_with('/') {
                            format!("{}{}", root_url, parameters[0])
                        } else {
                            parameters[0].clone()
                        };
                        let mut image_tag = String::from("<img src=\"");
                        image_tag.push_str(&src_path);
                        image_tag.push('"');
                        if !weak_carry.is_empty() {
                            let tags: Vec<_> = weak_carry.drain(..).collect();
                            for weak_carryover in tags {
                                image_tag.push(' ');
                                image_tag.push_str(&weak_carryover_attribute(weak_carryover));
                            }
                        }
                        image_tag.push_str("/>");
                        image_tag
                    }
                    _ => {
                        // FIXME: add Infirm tags support, we are currently ignoring them
                        info!("[converter] InfirmTag: {:#?}", self);
                        todo!()
                    }
                }
            }
            NorgAST::DelimitingModifier(t) => {
                if *t == DelimitingModifier::HorizontalRule {
                    let mut hr_tag = String::from("<hr");
                    if !weak_carry.is_empty() {
                        let tags: Vec<_> = weak_carry.drain(..).collect();
                        for weak_carryover in tags {
                            hr_tag.push(' ');
                            hr_tag.push_str(&weak_carryover_attribute(weak_carryover));
                        }
                    }
                    hr_tag.push_str("/>");
                    hr_tag
                } else {
                    // XXX: support weak and strong delimiting modifiers?
                    error!("[converter] {:#?}", self);
                    todo!()
                }
            }
            NorgAST::List {
                modifier_type,
                items,
            } => match modifier_type {
                NestableDetachedModifier::UnorderedList | NestableDetachedModifier::OrderedList => {
                    let list_open = get_list_tag(*modifier_type, true);
                    let list_close = get_list_tag(*modifier_type, false);
                    let mut list = list_open;
                    list.push_str(&to_html(items, strong_carry, &weak_carry, root_url, toc));
                    list.push_str(&list_close);
                    list
                }
                _ => to_html(items, strong_carry, &weak_carry, root_url, toc),
            },
            _ => {
                info!("[converter] {:#?}", self);
                todo!() // Fail on stuff that we cannot parse yet
            }
        }
    }
}

fn to_html(
    ast: &[NorgAST],
    strong_carry: &[CarryOverTag],
    weak_carry: &VecDeque<CarryOverTag>,
    root_url: &str,
    toc: &mut Vec<TocEntry>,
) -> String {
    let mut res = String::new();
    for node in ast {
        res.push_str(&node.to_html(strong_carry, weak_carry.clone(), root_url, toc));
    }

    res
}

/// Convert TOC entries to TOML
pub fn toc_to_toml(toc: &[TocEntry]) -> toml::Value {
    let mut items = toml::value::Array::new();

    for entry in toc {
        let mut table = toml::value::Table::new();
        table.insert("level".into(), toml::Value::Integer(entry.level as i64));
        table.insert("title".into(), toml::Value::String(entry.title.clone()));
        table.insert("id".into(), toml::Value::String(entry.id.clone()));
        items.push(toml::Value::Table(table));
    }

    toml::Value::Array(items)
}

pub fn convert(document: &str, root_url: &str) -> (String, Vec<TocEntry>) {
    let ast = parse_tree(document).unwrap();
    let mut toc = Vec::<TocEntry>::new();
    // We do not have any carryover tag when starting to convert the document
    let html = to_html(&ast, &[], &VecDeque::new(), root_url, &mut toc);

    (html, toc)
}
