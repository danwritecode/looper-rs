use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tera::Value;
use tokio::sync::Mutex;

use crate::{looper::Looper, tools::{LooperTool, LooperTools}, types::{Handlers, LooperToolDefinition}};

pub struct SubAgentTool {
    tools: Arc<Mutex<dyn LooperTools>>,
}

impl SubAgentTool {
    pub fn new(tools: Arc<Mutex<dyn LooperTools>>) -> Self {
        SubAgentTool { tools }
    }
}

#[async_trait]
impl LooperTool for SubAgentTool {
    fn get_tool_name(&self) -> String { "spawn_sub_agent".to_string() }

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

    async fn execute(&self, args: &Value) -> Value {
        let Some(task_description) = args["task_description"]
            .as_str()
        else {
            return json!({ "error": "Missing 'task_description' argument" });
        };

        println!("Creating new looper instance in sub-agent");

        let mut looper = match Looper::builder(Handlers::OpenAIResponses("gpt-5.4"))
            .instructions("You're being used as a CLI example for an agent loop. Be succinct yet friendly and helpful.")
            .tools(self.tools.clone())
            .enable_sub_agents(false) // explicitly disabling sub_agents to avoid infinite looper
            .build().await 
        {
            Ok(l) => l,
            Err(e) => return json!({ "error": format!("An error occured when building Looper | Error: {}", e) })
        };

        println!("Sending the task description to looper");

        let result = match looper.send(&task_description).await {
            Ok(r) => r,
            Err(e) => return json!({ "error": format!("An error occured when sending message | Error: {}", e) })
        };

        println!("Got final text responding back out");

        match &result.final_text {
            Some(ft) => json!({ "agent_findings": ft }),
            None => json!({ "error": "No agent_findings output were generated" }),
        }
    }
}
