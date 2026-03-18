use crate::types::turn::{ThinkingBlock, TurnStep};
use gemini_rust::GenerationResponse;

impl From<GenerationResponse> for TurnStep {
    fn from(response: GenerationResponse) -> Self {
        let mut thinking = Vec::new();
        let mut text = None;
        let tool_calls = Vec::new();

        for candidate in &response.candidates {
            if let Some(parts) = &candidate.content.parts {
                for part in parts {
                    match part {
                        gemini_rust::Part::Text {
                            text: t, thought, ..
                        } => {
                            if *thought == Some(true) {
                                thinking.push(ThinkingBlock { content: t.clone() });
                            } else {
                                text = Some(t.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        TurnStep {
            thinking,
            text,
            tool_calls,
        }
    }
}
