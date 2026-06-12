mod search;

pub use search::SearchEmailsTool;

use rig::tool::Tool;

/// A wrapper around a [`Tool`] that implements [`Clone`].
#[derive(Clone)]
pub struct ToolWrapper<T: Tool + 'static>(Box<T>);

impl<T: Tool + 'static> ToolWrapper<T> {
    /// Creates a new [`ToolWrapper`] with the given `struct` that implements the [`Tool`] trait.
    ///
    /// example:
    /// ```rust,ignore
    /// use rpc_agent::tools::ToolWrapper;
    /// use rig::tool::Tool;
    ///
    /// struct MyTool;
    ///
    /// impl Tool for MyTool {
    ///     const NAME: &'static str = "my_tool";
    ///     type Error = anyhow::Error;
    ///     type Args = Input;
    ///     type Output = u32;
    ///
    ///     async fn definition(&self, _prompt: String) -> rig::completion::ToolDefinition {
    ///         ToolDefinition {
    ///             name: "get_ticket_price".to_string(),
    ///             description: "Get the price of a return ticket to the destination city.".to_string(),
    ///             parameters: serde_json::json!({
    ///                 "type": "object",
    ///                 "properties": {
    ///                     "destination_city": {
    ///                         "type": "string",
    ///                         "description": "The destination city"
    ///                     }
    ///                 },
    ///                 "required": ["destination_city"],
    ///             }),
    ///         }
    ///     }
    ///
    ///     async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
    ///         println!("Tools called for {}", &args.destination_city);
    ///         let result = Self::get_ticket_price(&args.destination_city)?;
    ///         Ok(result)
    ///     }
    /// }
    ///
    /// let tool = ToolWrapper::new(MyTool);
    /// ```
    pub fn new(tool: T) -> Self {
        Self(Box::new(tool))
    }

    pub(crate) fn tool(self) -> Box<T> {
        self.0
    }
}
