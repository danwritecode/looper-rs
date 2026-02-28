use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{tools::LooperTool, types::LooperToolDefinition};

#[derive(Default)]
pub struct SetAgentLoopStateTool;

#[async_trait]
impl LooperTool for SetAgentLoopStateTool {
    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .set_name("set_agent_loop_state")
            .set_description("You will use this to signal to the agent loop when you want to continue or when to finish your turn. This means that you can choose to continue so that you have the opportunity to use more tools calls even after responding to a user.")
            .set_paramters(json!({
                "type": "object",
                    "properties": {
                        "state": { 
                            "type": "string", 
                            "enum": [ "done", "continue" ]
                        },
                        "continue_reason": { "type": "string", "description": "If state == 'continue', then provide the continue reason which should be the work you want to accomplish in the next loop iteration." }
                    },
                    "required": ["state"]
            }))
    }

    async fn execute(&self, args: &Value) -> Value {
        let reason = args["continue_reason"].to_string();
        return json!({ "State": format!("continuing with reason: {}", reason) });
    }
}
