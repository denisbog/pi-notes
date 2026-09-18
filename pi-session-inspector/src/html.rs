//! HTML session report generator.
//!
//! Generates a self-contained (no external dependencies) HTML report for one or
//! more sessions. It renders a per-request histogram of fresh (non-cached) input
//! tokens with markers where the prompt cache was lost, plus a full timeline of
//! every assistant request (thinking, text, tool calls and results) so a user can
//! click a bar / marker and jump straight to the request that caused a large input
//! spike or invalidated the cache.

use crate::models::*;
use crate::scanner::{CacheTracker, SessionSummary};
use serde_json::{json, Value};
use std::path::Path;

/// A single model request (assistant message) rendered in the timeline/histogram.
pub struct RequestItem {
    pub idx: usize,
    pub timestamp: String,
    pub model: Option<String>,
    pub stop: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    pub total: u64,
    pub cost_total: f64,
    pub invalidated: bool,
    pub lost_tokens: u64,
    pub text: String,
    pub thinking: String,
    pub tool_calls: Vec<ToolCallItem>,
}

/// A tool call issued by a request, with whatever results were attached to it.
pub struct ToolCallItem {
    pub name: String,
    pub args: Value,
    pub results: Vec<String>,
}

fn block_text(blocks: &[ContentBlock]) -> (String, String, Vec<(String, Value)>) {
    let mut text = String::new();
    let mut thinking = String::new();
    let mut calls = Vec::new();
    for b in blocks {
        match b {
            ContentBlock::Text { text: t } => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(t);
            }
            ContentBlock::Thinking { thinking: t, .. } => {
                if !thinking.is_empty() {
                    thinking.push('\n');
                }
                thinking.push_str(t);
            }
            ContentBlock::ToolCall { name, arguments, .. } => {
                calls.push((name.clone(), arguments.clone()));
            }
            _ => {}
        }
    }
    (text, thinking, calls)
}

fn result_text(blocks: &[ContentBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        if let ContentBlock::Text { text: t } = b {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(t);
        }
    }
    out
}

/// Build the ordered list of requests for a session, feeding a [`CacheTracker`]
/// in file order (matching the aggregate scan) so each request is flagged if it
/// invalidated the cache.
pub fn assemble_requests(entries: &[SessionEntry]) -> Vec<RequestItem> {
    // First pass: collect assistant messages in order and map tool results to
    // their parent assistant entry id.
    let mut assistants: Vec<(&MessageEntry, &AssistantMessage)> = Vec::new();
    let mut tool_results: std::collections::HashMap<String, Vec<&ToolResultMessage>> =
        std::collections::HashMap::new();

    for entry in entries {
        if let SessionEntry::Message(msg) = entry {
            if let AgentMessage::Assistant(assistant) = &msg.message {
                assistants.push((msg, assistant));
            } else if let AgentMessage::ToolResult(tr) = &msg.message {
                if let Some(parent) = &msg.parent_id {
                    tool_results.entry(parent.clone()).or_default().push(tr);
                }
            }
        }
    }

    // Second pass: walk entries again in file order to reproduce the exact cache
    // state transitions (assistant usage and tool-result usage both advance it).
    let mut tracker = CacheTracker::default();
    let obs_by_id: std::collections::HashMap<String, crate::scanner::CacheObservation> = {
        let mut m = std::collections::HashMap::new();
        for entry in entries {
            if let SessionEntry::Message(msg) = entry {
                match &msg.message {
                    AgentMessage::Assistant(a) => {
                        if let Some(usage) = &a.usage {
                            let obs = tracker.record(usage);
                            m.insert(msg.id.clone(), obs);
                        }
                    }
                    AgentMessage::ToolResult(tr) => {
                        if let Some(usage) = &tr.usage {
                            let _ = tracker.record(usage);
                        }
                    }
                    _ => {}
                }
            }
        }
        m
    };

    let mut requests = Vec::new();
    for (idx, (msg, assistant)) in assistants.iter().enumerate() {
        let (text, thinking, calls) = block_text(&assistant.content);
        let obs = obs_by_id
            .get(&msg.id)
            .copied()
            .unwrap_or(crate::scanner::CacheObservation {
                is_invalidation: false,
                lost_tokens: 0,
            });

        let tool_calls = calls
            .iter()
            .map(|(name, args)| {
                let results = tool_results
                    .get(&msg.id)
                    .map(|trs| trs.iter().map(|tr| result_text(&tr.content)).collect())
                    .unwrap_or_default();
                ToolCallItem {
                    name: name.clone(),
                    args: args.clone(),
                    results,
                }
            })
            .collect();

        let usage = &assistant.usage;
        requests.push(RequestItem {
            idx: idx + 1,
            timestamp: msg.timestamp.clone(),
            model: assistant.model.clone(),
            stop: assistant.stop_reason.clone().unwrap_or_default(),
            input: usage.as_ref().map(|u| u.input).unwrap_or(0),
            output: usage.as_ref().map(|u| u.output).unwrap_or(0),
            cache_read: usage.as_ref().map(|u| u.cache_read).unwrap_or(0),
            cache_write: usage.as_ref().map(|u| u.cache_write).unwrap_or(0),
            reasoning: usage.as_ref().map(|u| u.reasoning).unwrap_or(0),
            total: usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
            cost_total: usage.as_ref().map(|u| u.cost.total).unwrap_or(0.0),
            invalidated: obs.is_invalidation,
            lost_tokens: obs.lost_tokens,
            text,
            thinking,
            tool_calls,
        });
    }
    requests
}

/// Serialize one session (summary + requests) as a JSON object for the embedded
/// data blob in the HTML page.
fn session_json(summary: &SessionSummary, requests: &[RequestItem]) -> Value {
    json!({
        "sessionId": summary.session_id,
        "file": summary.session_file.to_string_lossy(),
        "project": summary.cwd,
        "started": summary.started_at,
        "durationSecs": summary.duration_secs,
        "name": summary.display_name,
        "input": summary.input_tokens,
        "cached": summary.cached_input_tokens,
        "output": summary.output_tokens,
        "reasoning": summary.reasoning_tokens,
        "total": summary.total_tokens,
        "cost": summary.total_cost,
        "invalidations": summary.cache_invalidations,
        "lostTokens": summary.cache_lost_tokens,
        "lostCost": summary.cache_lost_cost,
        "contextEnd": summary.context_tokens_at_end,
        "requests": requests
            .iter()
            .map(|r| {
                json!({
                    "idx": r.idx,
                    "time": r.timestamp,
                    "model": r.model,
                    "stop": r.stop,
                    "input": r.input,
                    "output": r.output,
                    "cacheRead": r.cache_read,
                    "cacheWrite": r.cache_write,
                    "reasoning": r.reasoning,
                    "total": r.total,
                    "cost": r.cost_total,
                    "invalidated": r.invalidated,
                    "lost": r.lost_tokens,
                    "text": r.text,
                    "thinking": r.thinking,
                    "toolCalls": r.tool_calls
                        .iter()
                        .map(|t| json!({
                            "name": t.name,
                            "args": t.args,
                            "results": t.results,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// Generate a self-contained HTML report for the given sessions and write it to
/// `path`. Returns the number of sessions written.
pub fn write_report(path: &Path, summaries: &[SessionSummary]) -> std::io::Result<usize> {
    let mut sessions: Vec<Value> = Vec::new();
    for summary in summaries {
        let entries = crate::scanner::parse_entries(&summary.session_file)?;
        let requests = assemble_requests(&entries);
        sessions.push(session_json(summary, &requests));
    }

    let data_json = serde_json::to_string(&json!({"sessions": sessions})).unwrap_or_default();
    // Prevent `</script>` in the data from breaking out of the embedded script tag.
    let data_json = data_json.replace("</", "<\\/");

    let html = template(&data_json, sessions.len());
    std::fs::write(path, html)?;
    Ok(sessions.len())
}

/// Write the exact data model embedded in the HTML report as JSON.
///
/// The returned shape is `{"sessions": [ ... ]}`, where each session carries its
/// aggregate stats plus the ordered `requests` list (usage, thinking, text,
/// tool calls and results) used to render the histogram and timeline. This lets
/// terminal front-ends (e.g. the pi `/report-view` extension) render the same
/// information without scraping the HTML.
///
/// Returns the number of sessions written.
pub fn write_json(path: &Path, summaries: &[SessionSummary]) -> std::io::Result<usize> {
    let mut sessions: Vec<Value> = Vec::new();
    for summary in summaries {
        let entries = crate::scanner::parse_entries(&summary.session_file)?;
        let requests = assemble_requests(&entries);
        sessions.push(session_json(summary, &requests));
    }

    let data = serde_json::to_string_pretty(&json!({"sessions": sessions})).unwrap_or_default();
    std::fs::write(path, data)?;
    Ok(sessions.len())
}

fn template(data_json: &str, session_count: usize) -> String {
    let count_str = if session_count == 1 {
        "1 session".to_string()
    } else {
        format!("{} sessions", session_count)
    };
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>pi session inspector — {}</title>
<style>
:root {{
  --bg:#0f1116; --panel:#171a22; --panel2:#1d2130; --border:#2a2f3e;
  --text:#e6e8ef; --muted:#9aa1b5; --accent:#5b8cff; --danger:#ff5d73;
  --warn:#ffb454; --ok:#3ecf8e; --mono: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
}}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--bg); color:var(--text); font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif; }}
.wrap {{ max-width:1200px; margin:0 auto; padding:24px; }}
h1 {{ font-size:20px; margin:0 0 4px; }}
.sub {{ color:var(--muted); margin:0 0 18px; }}
a {{ color:var(--accent); text-decoration:none; }}
a:hover {{ text-decoration:underline; }}
.stats {{ display:flex; flex-wrap:wrap; gap:10px; margin:0 0 8px; }}
.stat {{ background:var(--panel); border:1px solid var(--border); border-radius:8px; padding:8px 14px; min-width:120px; }}
.stat .k {{ font-size:11px; color:var(--muted); text-transform:uppercase; letter-spacing:.04em; }}
.stat .v {{ font-size:17px; font-weight:600; margin-top:2px; }}
.stat .v.danger {{ color:var(--danger); }}
.stat .v.warn {{ color:var(--warn); }}
.stat .v.ok {{ color:var(--ok); }}
.sessionhead {{ margin:26px 0 6px; }}
.sessionhead h2 {{ margin:0 0 2px; font-size:16px; }}
.sessionhead .sfile {{ color:var(--muted); font-family:var(--mono); font-size:12px; word-break:break-all; }}
.controls {{ display:flex; gap:14px; align-items:center; margin:14px 0 8px; flex-wrap:wrap; }}
.controls label {{ font-size:12px; color:var(--muted); }}
.histpanel {{ background:var(--panel); border:1px solid var(--border); border-radius:10px; padding:14px; margin-bottom:10px; }}
.hist {{ display:flex; align-items:flex-end; gap:1px; height:150px; padding-top:20px; position:relative; }}
.hist-inner {{ display:flex; align-items:flex-end; gap:1px; height:120px; flex:1; min-width:0; }}
.bar {{ flex:1; min-width:1px; background:var(--accent); opacity:.45; cursor:pointer; border-radius:1px 1px 0 0; transition:opacity .1s; }}
.bar:hover {{ opacity:1; }}
.bar.large {{ background:var(--warn); opacity:.8; }}
.bar.loss {{ background:var(--danger); opacity:1; }}
.markers {{ display:flex; gap:1px; height:10px; margin-top:2px; }}
.mk {{ flex:1; min-width:1px; }}
.mk.loss {{ background:var(--danger); border-radius:1px; }}
.axis {{ display:grid; grid-template-columns:1fr 1fr; font-size:11px; color:var(--muted); margin-top:6px; }}
.histlegend {{ font-size:11px; color:var(--muted); display:flex; gap:16px; margin-top:8px; }}
.histlegend .sw {{ display:inline-block; width:12px; height:12px; border-radius:2px; margin-right:6px; vertical-align:-2px; }}
.tl {{ margin-top:10px; }}
.card {{ background:var(--panel); border:1px solid var(--border); border-left:3px solid var(--border); border-radius:8px; padding:10px 14px; margin-bottom:8px; scroll-margin-top:70px; transition:box-shadow .15s; }}
.card.flash {{ border-left-color:var(--accent); box-shadow:0 0 0 3px rgba(91,140,255,.25); }}
.card.loss {{ border-left-color:var(--danger); }}
.card.large {{ border-left-color:var(--warn); }}
.cardhead {{ display:flex; align-items:center; gap:10px; flex-wrap:wrap; }}
.cardhead .idx {{ font-family:var(--mono); color:var(--muted); }}
.cardhead .time {{ font-family:var(--mono); font-size:12px; color:var(--muted); }}
.cardhead .model {{ font-family:var(--mono); font-size:11px; color:var(--muted); }}
.cardhead .grow {{ flex:1; }}
.badge {{ font-size:10px; font-weight:700; letter-spacing:.05em; padding:2px 8px; border-radius:999px; text-transform:uppercase; }}
.badge.loss {{ background:var(--danger); color:#fff; }}
.badge.large {{ background:var(--warn); color:#1d1b14; }}
.chips {{ display:flex; gap:8px; flex-wrap:wrap; margin-top:6px; }}
.chip {{ font-family:var(--mono); font-size:11px; background:var(--panel2); border:1px solid var(--border); padding:2px 8px; border-radius:6px; }}
.chip b {{ color:var(--text); }}
.chip .hi {{ color:var(--warn); }}
.chip .lo {{ color:var(--danger); }}
.details {{ margin-top:8px; }}
.details details {{ margin-bottom:6px; }}
.details summary {{ cursor:pointer; color:var(--accent); font-size:12px; font-weight:600; }}
.pre {{ background:#0b0d13; border:1px solid var(--border); border-radius:6px; padding:10px; font-family:var(--mono); font-size:12px; white-space:pre-wrap; word-break:break-word; max-height:260px; overflow:auto; margin:6px 0 0; }}
.toolcall {{ margin:6px 0 0; }}
.toolname {{ font-family:var(--mono); font-size:12px; color:var(--ok); }}
.toollabel {{ font-size:11px; color:var(--muted); }}
.empty {{ color:var(--muted); font-style:italic; }}
.problems {{ background:var(--panel); border:1px solid var(--border); border-radius:10px; padding:12px 14px; margin:6px 0 10px; border-color:var(--danger); }}
.problems h3 {{ margin:0 0 8px; font-size:13px; color:var(--danger); }}
.problems ul {{ margin:0; padding-left:18px; }}
.problems li {{ font-size:13px; margin-bottom:4px; font-family:var(--mono); }}
.problems .tag {{ color:var(--warn); }}
.problems .tagd {{ color:var(--danger); }}
.problems.large {{ border-color:var(--warn); }}
.problems.large h3 {{ color:var(--warn); }}
.dim {{ color:var(--muted); }}
html {{ scroll-behavior:smooth; }}
.nav {{ position:sticky; top:0; background:var(--bg); padding:8px 0; z-index:5; font-size:12px; }}
.nav a {{ margin-right:12px; }}
#index {{ display:flex; flex-wrap:wrap; gap:8px; margin:4px 0 22px; }}
.indexitem {{ display:inline-block; background:var(--panel); border:1px solid var(--border); border-radius:8px; padding:7px 12px; font-size:13px; }}
.indexitem:hover {{ border-color:var(--accent); }}
.indexitem b {{ color:var(--text); }}
.tagd {{ color:var(--danger); }}
.totop {{ color:var(--muted); font-size:11px; text-decoration:none; font-weight:600; }}
.totop:hover {{ color:var(--accent); }}
.bcount {{ color:var(--muted); font-family:var(--mono); font-size:11px; margin-left:6px; }}
.nxmark {{ color:var(--accent); font-size:10px; font-weight:600; margin-left:8px; text-transform:uppercase; letter-spacing:.03em; }}
.nxmark.note {{ color:var(--muted); }}
.nextcall {{ background:rgba(94,140,255,.10); border:1px dashed var(--accent); border-radius:6px; padding:6px 10px; margin-top:8px; font-size:12px; }}
.nextcall b {{ color:var(--text); }}
.nxlabel {{ font-size:10px; font-weight:700; letter-spacing:.05em; color:var(--accent); text-transform:uppercase; margin-right:6px; border:1px solid var(--accent); border-radius:4px; padding:0 5px; }}

</style>
</head>
<body>
<div class="wrap">
  <div id="top"></div>
  <h1>pi-session-inspector — token &amp; cache report</h1>
  <p class="sub">{count_str} · hover a histogram bar for details; click bar or marker to jump to the request · red markers = cache lost</p>
  <div id="index"></div>
  <div id="sessions"></div>
</div>

<script id="data" type="application/json">{data_json}</script>
<script>
(function () {{
  var DATA;
  try {{ DATA = JSON.parse(document.getElementById('data').textContent); }}
  catch (e) {{ document.getElementById('sessions').innerHTML = '<p class="empty">Failed to parse embedded data.</p>'; return; }}

  function fmt(n) {{
    if (n == null) return '0';
    if (n >= 1000000) return (n/1000000).toFixed(1)+'M';
    if (n >= 1000) return (n/1000).toFixed(1)+'K';
    return ''+n;
  }}
  function fmtCost(n) {{
    if (n == null) return '0';
    if (n >= 1) return '$'+n.toFixed(4);
    if (n >= 0.01) return '$'+n.toFixed(4);
    return '$'+n.toFixed(6);
  }}
  function esc(s) {{
    return String(s==null?'':s)
      .replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;')
      .replace(/"/g,'&quot;');
  }}
  function pct(arr) {{
    var s=[].concat(arr).sort(function(a,b){{return a-b;}});
    return s[Math.floor(s.length*0.8)] || 0;
  }}

  function root() {{
    var d = document.createElement('div');
    return d;
  }}

  function appendStat(row, k, v, cls) {{
    var s = document.createElement('div'); s.className='stat';
    s.innerHTML = '<div class="k">'+esc(k)+'</div><div class="v '+(cls||'')+'">'+v+'</div>';
    row.appendChild(s);
  }}

  function histogram(sess, reqs, largeThresh, focusFn) {{
    var max = 1;
    reqs.forEach(function(r){{ if(r.input>max) max=r.input; }});
    var maxH = 120, minH = 3;

    var panel = document.createElement('div'); panel.className = 'histpanel';

    var inner = document.createElement('div'); inner.className='hist-inner';
    reqs.forEach(function(r, i) {{
      var b = document.createElement('div');
      b.className = 'bar'+(r.invalidated?' loss':(r.input>=largeThresh?' large':''));
      var h = minH + (maxH-minH) * (Math.log(1+r.input)/Math.log(1+max));
      b.style.height = h+'px';
      b.title = '#'+r.idx+' '+r.time+'\ninput '+fmt(r.input)+' · cached '+fmt(r.cacheRead)+
                '\n'+(r.invalidated?('CACHE LOST '+fmt(r.lost)+' tokens'):'')+
                '\nclick to focus request #'+r.idx;
      (function(i){{
        b.addEventListener('click', function(){{ focusFn(i); }});
      }})(i);
      inner.appendChild(b);
    }});
    panel.appendChild(inner);

    var markers = document.createElement('div'); markers.className='markers';
    reqs.forEach(function(r, i) {{
      var mk = document.createElement('div');
      mk.className = 'mk'+(r.invalidated?' loss':'');
      mk.title = r.invalidated ? '#'+r.idx+': cache lost '+fmt(r.lost)+' tokens (click to focus)' : '#'+r.idx;
      if (r.invalidated) mk.addEventListener('click', function(){{ focusFn(i); }});
      markers.appendChild(mk);
    }});
    panel.appendChild(markers);

    var lg = document.createElement('div'); lg.className='histlegend';
    lg.innerHTML =
      '<span><span class="sw" style="background:var(--accent)"></span>normal</span>'+
      '<span><span class="sw" style="background:var(--warn)"></span>large input</span>'+
      '<span><span class="sw" style="background:var(--danger)"></span>cache lost (red marker)</span>';
    panel.appendChild(lg);
    return panel;
  }}

  function toks(chars) {{
    return Math.round(chars / 4);
  }}
  function reqCard(r, largeThresh, sessIdx) {{
    var c = document.createElement('div');
    c.className = 'card'+(r.invalidated?' loss':(r.input>=largeThresh?' large':''));
    c.id = 'req-'+sessIdx+'-'+r.idx;
    c.dataset.idx = r.idx;

    var h = document.createElement('div'); h.className='cardhead';
    h.innerHTML =
      '<span class="idx">#'+r.idx+'</span>'+
      '<span class="time">'+esc(r.time)+'</span>'+
      '<span class="model">'+(r.model?esc(r.model):'')+'</span>'+
      '<span class="grow"></span>'+
      (r.invalidated?'<span class="badge loss">cache lost '+fmt(r.lost)+'</span>':'')+
      (r.input>=largeThresh?'<span class="badge large">large input</span>':'')+
      '<a class="totop" href="#sess-'+(sessIdx+1)+'" title="jump to current session overview">session overview</a>';
    c.appendChild(h);

    var chips = document.createElement('div'); chips.className='chips';
    var chipHtml =
      '<span class="chip">input <b'+(r.input>=largeThresh?' class="hi"':'')+'>'+fmt(r.input)+'</b></span>'+
      '<span class="chip">cached <b>'+fmt(r.cacheRead)+'</b></span>'+
      '<span class="chip">output <b>'+fmt(r.output)+'</b></span>'+
      (r.reasoning?'<span class="chip">reasoning <b>'+fmt(r.reasoning)+'</b></span>':'')+
      '<span class="chip">total <b>'+fmt(r.total)+'</b></span>'+
      '<span class="chip">$ <b>'+fmtCost(r.cost)+'</b></span>'+
      (r.stop?'<span class="chip">'+esc(r.stop)+'</span>':'');
    chips.innerHTML = chipHtml;
    c.appendChild(chips);

    // Marker: what is considered for the NEXT call. The model's output this turn
    // (thinking + text + tool-call args) is appended to the conversation and is
    // re-sent as fresh input on the following request.
    var outChars = (r.thinking?r.thinking.length:0)+(r.text?r.text.length:0);
    (r.toolCalls||[]).forEach(function(t){{
      var s='';
      try {{ s=JSON.stringify(t.args); }} catch(e) {{ s=''+t.args; }}
      outChars += s.length;
    }});
    var nx = document.createElement('div'); nx.className='nextcall';
    nx.innerHTML = '<span class="nxlabel">next call</span>'+
      ' output <b>'+fmt(outChars)+'</b> chars / <b>'+fmt(toks(outChars))+'</b> est. tokens'+
      (r.reasoning?' (incl. '+fmt(r.reasoning)+' reasoning)':'')+
      ' is appended to the conversation and re-sent as input on the next request.';
    c.appendChild(nx);

    var d = document.createElement('div'); d.className='details';

    if (r.thinking) {{
      var dt = document.createElement('details');
      dt.innerHTML = '<summary>thinking <span class="bcount">'+fmt(r.thinking.length)+' chars · '+fmt(toks(r.thinking.length))+' tok</span><span class="nxmark">next call</span></summary><pre class="pre"></pre>';
      dt.querySelector('.pre').textContent = r.thinking;
      d.appendChild(dt);
    }}
    if (r.text) {{
      var dt2 = document.createElement('details');
      dt2.open = true;
      dt2.innerHTML = '<summary>assistant text <span class="bcount">'+fmt(r.text.length)+' chars · '+fmt(toks(r.text.length))+' tok</span><span class="nxmark">next call</span></summary><pre class="pre"></pre>';
      dt2.querySelector('.pre').textContent = r.text;
      d.appendChild(dt2);
    }}
    (r.toolCalls||[]).forEach(function(t) {{
      var tc = document.createElement('div'); tc.className='toolcall';
      var argsStr='';
      try {{ argsStr = JSON.stringify(t.args, null, 2); }} catch(e) {{ argsStr = ''+t.args; }}
      var head = document.createElement('div');
      head.innerHTML = '<span class="toolname">'+esc(t.name)+'</span> <span class="toollabel">tool call</span> '+
        '<span class="bcount">'+fmt(argsStr.length)+' chars · '+fmt(toks(argsStr.length))+' tok</span>'+
        '<span class="nxmark">next call</span>';
      tc.appendChild(head);
      var apre = document.createElement('pre'); apre.className='pre';
      apre.textContent = argsStr;
      tc.appendChild(apre);
      if (t.results && t.results.length) {{
        var rdet = document.createElement('details');
        var resLen = (t.results.join('')).length;
        rdet.innerHTML = '<summary>tool result <span class="bcount">'+fmt(resLen)+' chars · '+fmt(toks(resLen))+' tok</span><span class="nxmark note">added by tool</span></summary>';
        t.results.forEach(function(rr) {{
          var pre = document.createElement('pre'); pre.className='pre';
          pre.textContent = rr; rdet.appendChild(pre);
        }});
        tc.appendChild(rdet);
      }}
      d.appendChild(tc);
    }});
    if (!r.thinking && !r.text && !(r.toolCalls||[]).length) {{
      var emp = document.createElement('div'); emp.className='empty'; emp.textContent='(no content)';
      d.appendChild(emp);
    }}
    c.appendChild(d);
    return c;
  }}

  function cacheLossTimeline(sess, reqs, focusFn) {{
    var losses = [];
    var prevCr = null;
    var prevT = null;
    // Cost per lost token derived from the session totals, so each loss event can
    // be priced at the same effective input-vs-cache-read rate used for the total.
    var lossCostRate = (sess.lostCost && sess.lostTokens) ? sess.lostCost / sess.lostTokens : 0;
    for (var i = 0; i < reqs.length; i++) {{
      var r = reqs[i];
      var gap = null;
      var t = new Date(r.time).getTime();
      if (prevT !== null) gap = (t - prevT) / 1000;
      var dropPct = null;
      if (prevCr !== null && prevCr > 0) dropPct = (prevCr - r.cacheRead) / prevCr * 100;
      if (r.invalidated && dropPct !== null && dropPct > 0.01) {{
        losses.push({{i:i, r:r, gap:gap, prevCr:prevCr, dropPct:dropPct, cost: r.lost * lossCostRate}});
      }}
      prevCr = r.cacheRead;
      prevT = t;
    }}
    if (!losses.length) return null;
    var box = document.createElement('div'); box.className='problems';
    box.innerHTML = '<h3>Cache-loss timeline ('+losses.length+' · '+fmtCost(sess.lostCost||0)+')</h3><ul></ul>';
    var ul = box.querySelector('ul');
    losses.forEach(function(l) {{
      var li = document.createElement('li');
      li.innerHTML = '<a href="#" data-focus="'+l.i+'" class="tagd">#'+l.r.idx+'</a> '+
        '<span class="dim">'+esc(l.r.time)+' · gap '+(l.gap!=null?l.gap.toFixed(0)+'s':'-')+
        ' · cache '+fmt(l.prevCr)+' → '+fmt(l.r.cacheRead)+' ('+(l.dropPct.toFixed(0))+')%'+
        ' · re-sent <b>'+fmt(l.r.input)+'</b> tokens · cost <b class="tagd">'+fmtCost(l.cost)+'</b></span>';
      ul.appendChild(li);
    }});
    box.addEventListener('click', function(ev){{
      var t = ev.target.closest('[data-focus]');
      if (t) {{ ev.preventDefault(); focusFn(+t.getAttribute('data-focus')); }}
    }});
    return box;
  }}

  function sessionBlock(sess, idx) {{ 
    var reqs = sess.requests || [];
    var largeThresh = Math.max(500, pct(reqs.map(function(r){{return r.input;}})));

    var wrap = document.createElement('section');
    wrap.id = 'sess-'+(idx+1);
    var sh = document.createElement('div'); sh.className='sessionhead';
    sh.innerHTML = '<h2>'+esc(sess.sessionId||'session')+' <a class="totop" href="#top" title="jump to top of page">(top)</a></h2>'+
                   '<div class="sfile">'+esc(sess.file||'')+'</div>';
    wrap.appendChild(sh);

    var stats = document.createElement('div'); stats.className='stats';
    appendStat(stats,'input',fmt(sess.input));
    appendStat(stats,'cached',fmt(sess.cached));
    appendStat(stats,'output',fmt(sess.output));
    appendStat(stats,'requests',''+reqs.length);
    var largeCount = reqs.filter(function(r){{ return r.input >= largeThresh; }}).length;
    appendStat(stats,'large input',''+largeCount, largeCount?'warn':'');
    appendStat(stats,'cache lost events',''+(sess.invalidations||0), (sess.invalidations?'danger':''));
    appendStat(stats,'tokens lost',fmt(sess.lostTokens||0), (sess.lostTokens?'warn':''));
    appendStat(stats,'cost of lost tokens',fmtCost(sess.lostCost||0), (sess.lostCost?'danger':''));
    appendStat(stats,'context at end',fmt(sess.contextEnd||0));
    appendStat(stats,'cost',fmtCost(sess.cost));
    wrap.appendChild(stats);

    // Large-request quick-list: the top fresh-input requests only (no cache
    // invalidations — those are covered separately by the cache-loss timeline).
    var flagged = [];
    var seen = {{}};
    function add(f) {{ if (seen[f.i]) return; seen[f.i]=true; flagged.push(f); }}
    var top = reqs.map(function(r,i){{return {{r:r,i:i}};}})
      .sort(function(a,b){{ return b.r.input-a.r.input; }}).slice(0,5);
    top.forEach(function(t){{ add({{r:t.r,i:t.i,label:'input '+fmt(t.r.input)}}); }});
    if (flagged.length) {{
      var pb = document.createElement('div'); pb.className='problems large';
      pb.innerHTML = '<h3>Large requests</h3><ul></ul>';
      var ul = pb.querySelector('ul');
      flagged.forEach(function(f) {{
        var li = document.createElement('li');
        li.innerHTML = '<a href="#" data-focus="'+f.i+'" class="tag">#'+f.r.idx+'</a> '+
          '<span class="dim">'+esc(f.r.time)+' · '+f.label+'</span>';
        ul.appendChild(li);
      }});
      pb.addEventListener('click', function(ev) {{
        var t = ev.target.closest('[data-focus]');
        if (t) {{ ev.preventDefault(); focusReq(idx, +t.getAttribute('data-focus')); }}
      }});
      wrap.appendChild(pb);
    }}

    var hist = histogram(sess, reqs, largeThresh, function(i){{ focusReq(idx, i); }});
    var cl = cacheLossTimeline(sess, reqs, function(i){{ focusReq(idx, i); }});
    if (cl) wrap.appendChild(cl);
    wrap.appendChild(hist);

    var tl = document.createElement('div'); tl.className='tl';
    reqs.forEach(function(r){{ tl.appendChild(reqCard(r, largeThresh, idx)); }});
    wrap.appendChild(tl);
    return wrap;
  }}

  function focusReq(sessIdx, i) {{
    var card = document.getElementById('req-'+sessIdx+'-'+(i+1));
    if (!card) return;
    card.scrollIntoView({{behavior:'smooth', block:'start'}});
    card.classList.remove('flash');
    void card.offsetWidth;
    card.classList.add('flash');
    setTimeout(function(){{ card.classList.remove('flash'); }}, 1600);
  }}

  var root = document.getElementById('sessions');
  var idxBox = document.getElementById('index');
  (DATA.sessions||[]).forEach(function(s, i) {{
    if (idxBox) {{
      var a = document.createElement('a');
      a.className = 'indexitem';
      a.href = '#sess-'+(i+1);
      a.innerHTML = '<b>'+esc(s.name||s.sessionId||('session '+(i+1)))+'</b> '+
        '<span class="dim">'+fmt(s.input)+' in | '+fmt(s.cached)+' cached | '+fmt(s.contextEnd||0)+' ctx-end | '+s.requests.length+' req'+
        (s.invalidations ? ' | <span class="tagd">'+s.invalidations+' cache loss ('+fmt(s.lostTokens)+' tok · '+fmtCost(s.lostCost||0)+')</span>' : '')+
        '</span>';
      idxBox.appendChild(a);
    }}
    root.appendChild(sessionBlock(s, i));
  }});
  if (!(DATA.sessions||[]).length) root.innerHTML = '<p class="empty">No sessions in report.</p>';
}})();
</script>
</body>
</html>"##,
        "pi-session-inspector — token & cache report".to_string(),
        count_str = count_str,
        data_json = data_json,
    )
}
