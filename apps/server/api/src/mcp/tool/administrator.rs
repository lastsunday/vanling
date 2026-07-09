use chrono::{FixedOffset, Utc};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, InitializeResult, ServerCapabilities},
    schemars, tool, tool_handler, tool_router,
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SumRequest {
    #[schemars(description = "the left hand side number")]
    pub a: f64,
    pub b: f64,
}

#[derive(Debug, Clone)]
pub struct Administrator {
    #[expect(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl Administrator {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for Administrator {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl Administrator {
    #[tool(description = "Calculate the sum of two numbers")]
    fn sum(&self, Parameters(SumRequest { a, b }): Parameters<SumRequest>) -> String {
        (a + b).to_string()
    }

    #[tool(description = "Get current datetime")]
    fn datetime(&self) -> Result<CallToolResult, ErrorData> {
        let offset = FixedOffset::east_opt(8 * 60 * 60).unwrap();
        let datetime = Utc::now().with_timezone(&offset);
        Ok(CallToolResult::success(vec![ContentBlock::text(
            datetime.to_rfc3339(),
        )]))
    }
}

#[tool_handler]
impl ServerHandler for Administrator {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("A server administrator")
    }
}
