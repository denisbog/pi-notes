use serde::{Deserialize, Serialize};

// ── Session entry types ──

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
pub enum SessionEntry {
    #[serde(rename = "session")]
    SessionHeader(SessionHeader),
    #[serde(rename = "message")]
    Message(MessageEntry),
    #[serde(rename = "model_change")]
    ModelChange(ModelChangeEntry),
    #[serde(rename = "thinking_level_change")]
    ThinkingLevelChange(ThinkingLevelChangeEntry),
    #[serde(rename = "compaction")]
    Compaction(CompactionEntry),
    #[serde(rename = "branch_summary")]
    BranchSummary(BranchSummaryEntry),
    #[serde(rename = "custom")]
    Custom(CustomEntry),
    #[serde(rename = "custom_message")]
    CustomMessage(CustomMessageEntry),
    #[serde(rename = "label")]
    Label(LabelEntry),
    #[serde(rename = "session_info")]
    SessionInfo(SessionInfoEntry),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SessionHeader {
    pub version: Option<u32>,
    pub id: String,
    pub timestamp: String,
    pub cwd: String,
    #[serde(rename = "parentSession")]
    pub parent_session: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MessageEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub message: AgentMessage,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ModelChangeEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub provider: String,
    #[serde(rename = "modelId")]
    pub model_id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ThinkingLevelChangeEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    #[serde(rename = "thinkingLevel")]
    pub thinking_level: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CompactionEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub summary: String,
    #[serde(rename = "tokensBefore")]
    pub tokens_before: Option<u64>,
    #[serde(rename = "firstKeptEntryId")]
    pub first_kept_entry_id: Option<String>,
    pub usage: Option<Usage>,
    #[serde(rename = "retainedTail")]
    pub retained_tail: Option<Vec<AgentMessage>>,
    pub details: Option<serde_json::Value>,
    #[serde(rename = "fromHook")]
    pub from_hook: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BranchSummaryEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    #[serde(rename = "fromId")]
    pub from_id: String,
    pub summary: String,
    pub usage: Option<Usage>,
    pub details: Option<serde_json::Value>,
    #[serde(rename = "fromHook")]
    pub from_hook: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CustomEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    #[serde(rename = "customType")]
    pub custom_type: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CustomMessageEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    #[serde(rename = "customType")]
    pub custom_type: String,
    pub content: serde_json::Value,
    pub display: bool,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LabelEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    #[serde(rename = "targetId")]
    pub target_id: String,
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SessionInfoEntry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub timestamp: String,
    pub name: Option<String>,
}

// ── Agent message types ──

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "role")]
pub enum AgentMessage {
    #[serde(rename = "user")]
    User(UserMessage),
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    #[serde(rename = "toolResult")]
    ToolResult(ToolResultMessage),
    #[serde(rename = "bashExecution")]
    BashExecution(BashExecutionMessage),
    #[serde(rename = "custom")]
    CustomAgent(CustomAgentMessage),
    #[serde(rename = "branchSummary")]
    BranchSummaryMsg(BranchSummaryMessage),
    #[serde(rename = "compactionSummary")]
    CompactionSummary(CompactionSummaryMessage),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UserMessage {
    pub content: ContentUnion,
    pub timestamp: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AssistantMessage {
    pub content: Vec<ContentBlock>,
    pub api: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub usage: Option<Usage>,
    #[serde(rename = "stopReason")]
    pub stop_reason: Option<String>,
    #[serde(rename = "errorMessage")]
    pub error_message: Option<String>,
    pub timestamp: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolResultMessage {
    #[serde(rename = "toolCallId")]
    pub tool_call_id: String,
    #[serde(rename = "toolName")]
    pub tool_name: String,
    pub content: Vec<ContentBlock>,
    pub details: Option<serde_json::Value>,
    pub usage: Option<Usage>,
    #[serde(rename = "isError")]
    pub is_error: bool,
    pub timestamp: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BashExecutionMessage {
    pub command: Option<String>,
    pub output: Option<String>,
    #[serde(rename = "exitCode")]
    pub exit_code: Option<i32>,
    pub cancelled: Option<bool>,
    pub truncated: Option<bool>,
    #[serde(rename = "fullOutputPath")]
    pub full_output_path: Option<String>,
    #[serde(rename = "excludeFromContext")]
    pub exclude_from_context: Option<bool>,
    pub timestamp: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CustomAgentMessage {
    #[serde(rename = "customType")]
    pub custom_type: String,
    pub content: serde_json::Value,
    pub display: Option<bool>,
    pub details: Option<serde_json::Value>,
    pub timestamp: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BranchSummaryMessage {
    pub summary: String,
    #[serde(rename = "fromId")]
    pub from_id: String,
    pub timestamp: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CompactionSummaryMessage {
    pub summary: String,
    #[serde(rename = "tokensBefore")]
    pub tokens_before: u64,
    pub timestamp: u64,
}

// ── Content types ──

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum ContentUnion {
    String(String),
    BlockArray(Vec<ContentBlock>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(rename = "thinkingSignature")]
        thinking_signature: Option<String>,
    },
    #[serde(rename = "toolCall")]
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
}

// ── Usage ──

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    #[serde(rename = "cacheRead")]
    pub cache_read: u64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: u64,
    #[serde(default)]
    pub reasoning: u64,
    #[serde(rename = "totalTokens")]
    pub total_tokens: u64,
    pub cost: Cost,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Cost {
    pub input: f64,
    pub output: f64,
    #[serde(rename = "cacheRead")]
    pub cache_read: f64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: f64,
    pub total: f64,
}
