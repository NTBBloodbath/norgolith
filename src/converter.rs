// TODO(ntbbloodbath): move this converter to a separate rust library called norg-converter

use html_escape::encode_text_minimal_to_string;
use rust_norg::{
    parse_tree, NestableDetachedModifier, NorgAST, NorgASTFlat, ParagraphSegment,
    ParagraphSegmentToken,
};

/// Converts paragraph segment tokens to a String
fn paragraph_tokens_to_string(tokens: &[ParagraphSegmentToken]) -> String {
    let mut s = String::new();
    encode_text_minimal_to_string(
        tokens
            .iter()
            .map(|token| match token {
                ParagraphSegmentToken::Text(txt) => txt.clone(),
                ParagraphSegmentToken::Whitespace => String::from(" "),
                ParagraphSegmentToken::Special(c) => String::from(*c),
                ParagraphSegmentToken::Escape(c) => String::from(*c),
            })
            .collect::<Vec<String>>()
            .join(""),
        &mut s,
    );
    s
}

/// Converts a ParagraphSegment into a String
fn paragraph_to_string(segment: &[ParagraphSegment]) -> String {
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
            let mut tag = |name: &str| {
                paragraph.push_str(&format!(
                    "<{name}>{}</{name}>",
                    &paragraph_to_string(content)
                ))
            };
            match modifier_type {
                '*' => tag("strong"),
                '/' => tag("em"),
                '_' => tag("u"),
                '-' => tag("s"),
                '^' => tag("sup"),
                ',' => tag("sub"),
                '!' => paragraph.push_str(&format!(
                    "<span class='spoiler'>{}</span>",
                    &paragraph_to_string(content)
                )),
                '$' => tag("code"), // TODO: Real Math Rendering?
                '%' => {}, // ignore comments
                _ => {
                    println!(
                        "ParagraphSegment::AttachedModifier: {} {:?}",
                        modifier_type, content
                    );
                    todo!()
                }
            }
        }
        ParagraphSegment::InlineVerbatim(content) => {
            paragraph.push_str(dbg!(&format!(
                "<code>{}</code>",
                &paragraph_tokens_to_string(content)
            )));
        }
        // ParagraphSegment::AttachedModifierOpener(_) => todo!(),
        // ParagraphSegment::AttachedModifierOpenerFail(_) => todo!(),
        // ParagraphSegment::AttachedModifierCloserCandidate(_) => todo!(),
        // ParagraphSegment::AttachedModifierCloser(_) => todo!(),
        // ParagraphSegment::AttachedModifierCandidate { modifier_type, content, closer } => todo!(),
        // ParagraphSegment::Link { filepath, targets, description } => todo!(),
        // ParagraphSegment::AnchorDefinition { content, target } => todo!(),
        // ParagraphSegment::Anchor { content, description } => todo!(),
        // ParagraphSegment::InlineLinkTarget(_) => todo!(),
        _ => {
            println!("ParagraphSegment: {:?}", node);
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

trait NorgToHtml {
    fn to_html(&self) -> String;
}

impl NorgToHtml for NorgAST {
    // TODO: finish VerbatimRangedTag support, add support for carry over tags, footnotes (they are tricky in HTML), anything else that I'm missing
    fn to_html(&self) -> String {
        match self {
            NorgAST::Paragraph(s) => {
                let mut paragraph: String = "<p>".to_string();
                paragraph.push_str(&paragraph_to_string(s));
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

                match level {
                    1..=6 => section.push_str(
                        format!("<h{0}>{1}</h{0}>", level, paragraph_to_string(title)).as_str(),
                    ),
                    // XXX: fallback to h6 if the header level is higher than 6
                    _ => section
                        .push_str(format!("<h6>{}</h6>", paragraph_to_string(title)).as_str()),
                }
                // HACK: currently, rust-norg trims all trailing whitespaces and every single newline from the norg documents
                section.push('\n');
                section.push_str(&to_html(content));

                section
            }
            NorgAST::NestableDetachedModifier {
                modifier_type,
                level,
                text,
                content,
                ..
            } => {
                // HACK: 'text' is actually a 'Box<NorgASTFlat>' value. It should be converted into a `ParagraphSegment` later in the rust-norg parser
                let mod_text = if let NorgASTFlat::Paragraph(s) = *text.clone() {
                    paragraph_to_string(&s)
                } else {
                    unreachable!();
                };

                match modifier_type {
                    NestableDetachedModifier::UnorderedList
                    | NestableDetachedModifier::OrderedList => {
                        let mut list = String::new();
                        if *level == 1 {
                            list.push_str(&get_list_tag(modifier_type.clone(), true));
                        }
                        list.push_str(format!("<li>{}</li>", mod_text).as_str());
                        if !content.is_empty() {
                            list.push_str(&get_list_tag(modifier_type.clone(), true));
                            list.push_str(&to_html(content));
                            list.push_str(&get_list_tag(modifier_type.clone(), false));
                        }
                        if *level == 1 {
                            list.push_str(&get_list_tag(modifier_type.clone(), false));
                        }
                        list
                    }
                    NestableDetachedModifier::Quote => {
                        let mut quote = String::new();
                        quote.push_str("<blockquote>");
                        quote.push_str(mod_text.as_str());
                        if !content.is_empty() {
                            quote.push_str(&to_html(content));
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
                // HACK: why is name a vector?
                match name[0].as_str() {
                    "code" => {
                        // NOTE: the class `language-foo` is being added by default so the converter can work out-of-the-box
                        // with libraries like highlight.js or prismjs
                        verbatim_tag.push_str(
                            format!(
                                "<pre><code class=\"language-{}\">{}</code></pre>",
                                parameters[0], content
                            )
                            .as_str(),
                        )
                    }
                    // TODO: support other verbatim ranged tags like '@image', '@math'
                    _ => {
                        if name[0] != "document" {
                            println!("VerbatimRangedTag: {:?}", self);
                            todo!()
                        }
                    }
                }
                verbatim_tag
            }
            NorgAST::CarryoverTag { .. } => {
                // FIXME: add Carryover tags support, we are currently ignoring them
                println!("CarryoverTag: {:?}", self);
                "".to_string()
            }
            NorgAST::InfirmTag { .. } => {
                // FIXME: add Infirm tags support, we are currently ignoring them
                println!("InfirmTag: {:?}", self);
                "".to_string()
            }
            _ => {
                println!("{:?}", self);
                todo!() // Fail on stuff that we cannot parse yet
            }
        }
    }
}

fn to_html(ast: &[NorgAST]) -> String {
    let mut res = String::new();
    for node in ast {
        res.push_str(&node.to_html());
    }

    res
}

pub fn convert(document: String) -> String {
    let ast = parse_tree(&document).unwrap();
    to_html(&ast)
}
