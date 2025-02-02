// TODO(ntbbloodbath): move this converter to a separate rust library called norg-converter
//
// NOTE: the current carryover tags management is the worst boilerplate code I've ever written.
// Refactor later to abstract it even further and make the code cleaner.

use html_escape::encode_text_minimal_to_string;
use rust_norg::{
    parse_tree, CarryoverTag, NestableDetachedModifier, NorgAST, NorgASTFlat, ParagraphSegment,
    ParagraphSegmentToken,
};

/// CarryOver
#[derive(Clone, Debug)]
struct CarryOverTag {
    name: Vec<String>,
    parameters: Vec<String>,
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
                '%' => {}           // ignore comments
                _ => {
                    println!(
                        "[converter] ParagraphSegment::AttachedModifier: {} {:#?}",
                        modifier_type, content
                    );
                    todo!()
                }
            }
        }
        ParagraphSegment::InlineVerbatim(content) => {
            paragraph.push_str(&format!(
                "<code>{}</code>",
                &paragraph_tokens_to_string(content)
            ));
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
            println!("[converter] ParagraphSegment: {:#?}", node);
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
            eprintln!("[converter] Carryover tag with namespace 'html' is expected to have an attribute name (e.g. 'html.class')");
        } else if weak_carryover.name.len() >= 3 {
            eprintln!(
                "[converter] Carryover tag with namespace 'html' is expected to have only one attribute name (e.g. 'html.class'), '{}' provided",
                weak_carryover.name.join(".")
            )
        } else {
            let attr_name = weak_carryover.name[1].as_str();
            let values_sep = if attr_name == "style" { ";" } else { " " };

            attr.push_str(format!(
                "{}=\"{}\"",
                &weak_carryover.name[1],
                weak_carryover.parameters.join(values_sep)
            ).as_str());
        }
    }
    attr
}

trait NorgToHtml {
    fn to_html(&self, strong_carry: Vec<CarryOverTag>, weak_carry: Vec<CarryOverTag>) -> String;
}

impl NorgToHtml for NorgAST {
    // TODO: finish VerbatimRangedTag support, add support for strong carryover tags, footnotes (they are tricky in HTML), anything else that I'm missing
    fn to_html(
        &self,
        strong_carry: Vec<CarryOverTag>,
        mut weak_carry: Vec<CarryOverTag>,
    ) -> String {
        match self {
            NorgAST::Paragraph(s) => {
                let mut paragraph = Vec::<String>::new();
                paragraph.push("<p".to_string());
                if !weak_carry.is_empty() {
                    for weak_carryover in weak_carry.clone() {
                        paragraph.push(weak_carryover_attribute(weak_carryover));
                        // Remove the carryover tag after using it because its lifetime
                        // ended after invocating it
                        weak_carry.remove(0);
                    }
                }
                paragraph.push(format!(">{}</p>", paragraph_to_string(s)));
                paragraph.join(" ")
            }
            NorgAST::Heading {
                level,
                title,
                content,
                ..
            } => {
                let mut section = Vec::<String>::new();

                match level {
                    1..=6 => {
                        section.push(format!("<h{level}"));
                        if !weak_carry.is_empty() {
                            for weak_carryover in weak_carry.clone() {
                                section.push(weak_carryover_attribute(weak_carryover));
                                // Remove the carryover tag after using it because its lifetime
                                // ended after invocating it
                                weak_carry.remove(0);
                            }
                        }
                        section.push(format!(">{}</h{}>", paragraph_to_string(title), level));
                    }
                    // XXX: fallback to h6 if the header level is higher than 6
                    _ => {
                        section.push("<h6".to_string());
                        if !weak_carry.is_empty() {
                            for weak_carryover in weak_carry.clone() {
                                section.push(weak_carryover_attribute(weak_carryover));
                                // Remove the carryover tag after using it because its lifetime
                                // ended after invocating it
                                weak_carry.remove(0);
                            }
                        }
                        section.push(format!(">{}</h6>", paragraph_to_string(title)));
                    }
                }
                section.push(to_html(content, &strong_carry, &weak_carry));

                section.join(" ")
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
                        println!("{:#?}", &self);
                        let mut list = Vec::<String>::new();
                        if *level == 1 {
                            list.push(get_list_tag(modifier_type.clone(), true));
                        }
                        list.push("<li".to_string());
                        if !weak_carry.is_empty() {
                            for weak_carryover in weak_carry.clone() {
                                list.push(weak_carryover_attribute(weak_carryover));
                                // Remove the carryover tag after using it because its lifetime
                                // ended after invocating it
                                weak_carry.remove(0);
                            }
                        }
                        list.push(format!(">{}", mod_text));
                        list.push("</li>".to_string());
                        if !content.is_empty() {
                            list.push(get_list_tag(modifier_type.clone(), true));
                            list.push(to_html(content, &strong_carry, &weak_carry));
                            list.push(get_list_tag(modifier_type.clone(), false));
                        }
                        if *level == 1 {
                            list.push(get_list_tag(modifier_type.clone(), false));
                        }
                        list.join(" ")
                    }
                    NestableDetachedModifier::Quote => {
                        let mut quote = Vec::<String>::new();
                        quote.push("<blockquote".to_string());
                        if !weak_carry.is_empty() {
                            for weak_carryover in weak_carry.clone() {
                                quote.push(weak_carryover_attribute(weak_carryover));
                                // Remove the carryover tag after using it because its lifetime
                                // ended after invocating it
                                weak_carry.remove(0);
                            }
                        }
                        quote.push(mod_text);
                        if !content.is_empty() {
                            quote.push(to_html(content, &strong_carry, &weak_carry));
                        }
                        quote.push("</blockquote>".to_string());
                        quote.join(" ")
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
                        // TODO: add carryover tags support, this one is trigger for the class
                        // attribute because we are already setting a default value for it
                        //
                        // NOTE: the class `language-foo` is being added by default so the converter can work out-of-the-box
                        // with libraries like highlight.js or prismjs
                        verbatim_tag.push_str(
                            format!(
                                "<pre><code class=\"language-{}\">{}</code></pre>",
                                parameters[0], content
                            )
                            .as_str(),
                        );
                    }
                    // NOTE: this only works for base64 encoded images, regular images
                    // use the .image infirm tag.
                    "image" => {
                        let mut image_tag = Vec::<String>::new();
                        image_tag.push(format!("<img src=\"{}\"", content));
                        if !weak_carry.is_empty() {
                            for weak_carryover in weak_carry.clone() {
                                image_tag.push(weak_carryover_attribute(weak_carryover));
                                // Remove the carryover tag after using it because its lifetime
                                // ended after invocating it
                                weak_carry.remove(0);
                            }
                        }
                        image_tag.push("/>".to_string());
                        verbatim_tag = image_tag.join(" ");
                    }
                    // TODO: support other verbatim ranged tags like '@math'
                    _ => {
                        if name[0] != "document" {
                            println!("[converter] VerbatimRangedTag: {:#?}", self);
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
            } => {
                match tag_type {
                    CarryoverTag::Attribute => {
                        let tag = CarryOverTag {
                            name: name.clone(),
                            parameters: parameters.clone(),
                        };
                        weak_carry.push(tag);
                        to_html(&[*next_object.clone()], &strong_carry, &weak_carry)
                    }
                    CarryoverTag::Macro => {
                        eprintln!("[converter] Carryover tag macros are unsupported right now");
                        todo!()
                    }
                }
            }
            // InfirmTag: InfirmTag { name: ["image"], parameters: ["/assets/norgolith.svg", "Norgolith", "logo"] }
            NorgAST::InfirmTag { name, parameters } => {
                match name[0].as_str() {
                    "image" => {
                        let mut image_tag = Vec::<String>::new();

                        image_tag.push(format!("<img src=\"{}\"", parameters[0]));
                        if !weak_carry.is_empty() {
                            for weak_carryover in weak_carry.clone() {
                                image_tag.push(weak_carryover_attribute(weak_carryover));
                                // Remove the carryover tag after using it because its lifetime
                                // ended after invocating it
                                weak_carry.remove(0);
                            }
                        }

                        image_tag.push("/>".to_string());
                        image_tag.join(" ")
                    }
                    _ => {
                        // FIXME: add Infirm tags support, we are currently ignoring them
                        println!("[converter] InfirmTag: {:#?}", self);
                        todo!()
                    }
                }
            }
            _ => {
                println!("[converter] {:#?}", self);
                todo!() // Fail on stuff that we cannot parse yet
            }
        }
    }
}

fn to_html(ast: &[NorgAST], strong_carry: &[CarryOverTag], weak_carry: &[CarryOverTag]) -> String {
    let mut res = String::new();
    for node in ast {
        res.push_str(&node.to_html(strong_carry.to_vec(), weak_carry.to_vec()));
    }

    res
}

pub fn convert(document: String) -> String {
    let ast = parse_tree(&document).unwrap();
    // We do not have any carryover tag when starting to convert the document
    to_html(&ast, &[], &[])
}
