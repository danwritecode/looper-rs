use async_trait::async_trait;
use serde_json::json;
use tera::Value;

use crate::{looper::Looper, tools::LooperTool, types::LooperToolDefinition};

pub struct SubAgentTool {
    looper: Looper,
}

impl SubAgentTool {
    pub fn new(looper: Looper) -> Self {
        SubAgentTool { looper }
    }
}

#[async_trait]
impl LooperTool for SubAgentTool {
    fn get_tool_name(&self) -> String {
        "spawn_sub_agent".to_string()
    }

    fn tool(&self) -> LooperToolDefinition {
        LooperToolDefinition::default()
            .set_name("spawn_sub_agent")
            .set_description("
                Spawns a sub-agent to go and perform various tasks that report back with a high level summary to the caller. 
                Used to avoid pollution of the overall context window.

                *IMPORTANT*: The sub-agent has access to the exact set of tools that you have access to minus the sub-agent tool itself.
            ")
            .set_paramters(json!({
                "type": "object",
                "properties": {
                    "task_description": { "type": "string", "description": "A description of the task that the sub agent needs to perform." }
                },
                "required": ["task_description"]
            }))
    }

    async fn execute(&mut self, args: &Value) -> Value {
        let Some(task_description) = args["task_description"].as_str() else {
            return json!({ "error": "Missing 'task_description' argument" });
        };

        let result = match self.looper.send(&task_description).await {
            Ok(r) => r,
            Err(e) => {
                return json!({ "error": format!("An error occured when sending message | Error: {}", e) });
            }
        };

        match &result.final_text {
            Some(ft) => json!({ "agent_findings": ft }),
            None => json!({ "error": "No agent_findings output were generated" }),
        }
    }
}
