use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use serde_json::Value;

/// Represents the output of a tool execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ToolResult {
    /// The text content returned to the LLM.
    pub content: String,
    /// Indicates whether the tool execution encountered an error.
    pub is_error: bool,
}

/// The trait that all native and MCP-proxied tools must implement.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name of the tool (e.g., "read_file", "calculate").
    fn name(&self) -> &str;

    /// A clear description of what the tool does and when to use it.
    fn description(&self) -> &str;

    /// The JSON Schema defining the expected parameters.
    /// Typically formatted as `{"type": "object", "properties": {...}, "required": [...]}`.
    fn parameters(&self) -> Value;

    /// Executes the tool asynchronously with the given JSON arguments.
    async fn execute(&self, args: Value) -> ToolResult;
}

/// A registry that manages active tools and compiles their schemas for LLM ingestion.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Creates a new, empty ToolRegistry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Registers a tool in the registry. Overwrites if a tool with the same name exists.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Retrieves a tool by its unique name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Compiles all registered tools into LLM-compatible schemas.
    /// Returns a list of tool definitions compatible with the Anthropic/OpenAI tools parameter structure.
    pub fn to_llm_schemas(&self) -> Vec<Value> {
        let mut schemas: Vec<Value> = self
            .tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.parameters()
                })
            })
            .collect();
        
        // Sort by name to guarantee deterministic schema ordering in tests and requests.
        schemas.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        
        schemas
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // A mock tool that performs basic math addition
    struct AddTool;

    #[async_trait]
    impl Tool for AddTool {
        fn name(&self) -> &str {
            "add"
        }

        fn description(&self) -> &str {
            "Adds two numbers together."
        }

        fn parameters(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                },
                "required": ["a", "b"]
            })
        }

        async fn execute(&self, args: Value) -> ToolResult {
            let a = match args["a"].as_f64() {
                Some(val) => val,
                None => return ToolResult {
                    content: "Missing or invalid parameter 'a'".to_string(),
                    is_error: true,
                },
            };
            let b = match args["b"].as_f64() {
                Some(val) => val,
                None => return ToolResult {
                    content: "Missing or invalid parameter 'b'".to_string(),
                    is_error: true,
                },
            };
            ToolResult {
                content: format!("{}", a + b),
                is_error: false,
            }
        }
    }

    #[tokio::test]
    async fn test_tool_registration_and_retrieval() {
        let mut registry = ToolRegistry::new();
        let tool = Arc::new(AddTool);
        registry.register(tool.clone());

        let retrieved = registry.get("add");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "add");
        assert_eq!(retrieved.unwrap().description(), "Adds two numbers together.");

        let non_existent = registry.get("subtract");
        assert!(non_existent.is_none());
    }

    #[tokio::test]
    async fn test_tool_execution_success() {
        let tool = AddTool;
        let args = json!({ "a": 10.5, "b": 2.5 });
        let result = tool.execute(args).await;
        assert!(!result.is_error);
        assert_eq!(result.content, "13");
    }

    #[tokio::test]
    async fn test_tool_execution_error() {
        let tool = AddTool;
        let args = json!({ "a": 10.5 });
        let result = tool.execute(args).await;
        assert!(result.is_error);
        assert!(result.content.contains("parameter 'b'"));
    }

    #[tokio::test]
    async fn test_to_llm_schemas() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(AddTool));

        let schemas = registry.to_llm_schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["name"], "add");
        assert_eq!(schemas[0]["description"], "Adds two numbers together.");
        assert_eq!(schemas[0]["input_schema"]["type"], "object");
    }
}
