// Synthetic local wire replies; these are server fixtures, never SDK client calls.
import { sse, modelId } from "./pi-omp-wire-capture.mjs";
export const thought = "W0_SYNTHETIC_THOUGHT";
export const toolResult = "W0_LOCAL_READ_CONTENT";
export const errorMarker = "W0_SYNTHETIC_ERROR";
export const data = (value) => `data: ${JSON.stringify(value)}\n\n`;
export const event = (value) => `event: ${value.type}\n${data(value)}`;

function frames(protocol) {
  return sse(protocol)
    .split("\n\n")
    .filter(Boolean)
    .map((frame) => {
      const raw = frame
        .split("\n")
        .find((line) => line.startsWith("data: "))
        ?.slice(6);
      return raw === "[DONE]" ? raw : JSON.parse(raw);
    });
}
function encode(protocol, values) {
  return values
    .map((value) =>
      value === "[DONE]"
        ? "data: [DONE]\n\n"
        : protocol === "anthropic-messages" || protocol === "openai-responses"
          ? event(value)
          : data(value)
    )
    .join("");
}

export function thinkingSse(protocol) {
  const values = frames(protocol);
  if (protocol === "anthropic-messages") {
    for (const value of values) if ("index" in value) value.index++;
    values.splice(
      1,
      0,
      { type: "content_block_start", index: 0, content_block: { type: "thinking", thinking: "" } },
      {
        type: "content_block_delta",
        index: 0,
        delta: { type: "thinking_delta", thinking: thought },
      },
      {
        type: "content_block_delta",
        index: 0,
        delta: { type: "signature_delta", signature: "w0-fake-signature" },
      },
      { type: "content_block_stop", index: 0 }
    );
  } else if (protocol === "openai-completions") {
    values[0].choices[0].delta.reasoning_content = thought;
  } else if (protocol === "openai-responses") {
    const item = {
      type: "reasoning",
      id: "rs_w0",
      summary: [{ type: "summary_text", text: thought }],
    };
    for (const value of values) if ("output_index" in value) value.output_index++;
    values.splice(
      1,
      0,
      { type: "response.output_item.added", output_index: 0, item: { ...item, summary: [] } },
      {
        type: "response.reasoning_summary_part.added",
        item_id: item.id,
        output_index: 0,
        summary_index: 0,
        part: { type: "summary_text", text: "" },
      },
      {
        type: "response.reasoning_summary_text.delta",
        item_id: item.id,
        output_index: 0,
        summary_index: 0,
        delta: thought,
      },
      {
        type: "response.reasoning_summary_text.done",
        item_id: item.id,
        output_index: 0,
        summary_index: 0,
        text: thought,
      },
      {
        type: "response.reasoning_summary_part.done",
        item_id: item.id,
        output_index: 0,
        summary_index: 0,
        part: item.summary[0],
      },
      { type: "response.output_item.done", output_index: 0, item }
    );
    values.at(-1).response.output.unshift(item);
  } else values[0].candidates[0].content.parts.unshift({ text: thought, thought: true });
  return encode(protocol, values);
}

export function toolSse(protocol, name, path) {
  const args = { path };
  const json = JSON.stringify(args);
  if (protocol === "anthropic-messages")
    return encode(protocol, [
      frames(protocol)[0],
      {
        type: "content_block_start",
        index: 0,
        content_block: { type: "tool_use", id: "toolu_w0", name, input: {} },
      },
      {
        type: "content_block_delta",
        index: 0,
        delta: { type: "input_json_delta", partial_json: json },
      },
      { type: "content_block_stop", index: 0 },
      {
        type: "message_delta",
        delta: { stop_reason: "tool_use", stop_sequence: null },
        usage: { output_tokens: 5 },
      },
      { type: "message_stop" },
    ]);
  if (protocol === "openai-completions")
    return encode(protocol, [
      {
        id: "chatcmpl_w0",
        object: "chat.completion.chunk",
        created: 1,
        model: modelId,
        choices: [
          {
            index: 0,
            delta: {
              role: "assistant",
              tool_calls: [
                { index: 0, id: "call_w0", type: "function", function: { name, arguments: json } },
              ],
            },
            finish_reason: null,
          },
        ],
      },
      {
        id: "chatcmpl_w0",
        object: "chat.completion.chunk",
        created: 1,
        model: modelId,
        choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }],
        usage: { prompt_tokens: 7, completion_tokens: 5, total_tokens: 12 },
      },
      "[DONE]",
    ]);
  if (protocol === "openai-responses") {
    const item = {
      type: "function_call",
      id: "fc_w0",
      call_id: "call_w0",
      name,
      arguments: json,
      status: "completed",
    };
    const response = {
      id: "resp_w0",
      object: "response",
      created_at: 1,
      status: "completed",
      model: modelId,
      output: [item],
      usage: { input_tokens: 7, output_tokens: 5, total_tokens: 12 },
    };
    return encode(
      protocol,
      [
        { type: "response.created", response: { ...response, status: "in_progress", output: [] } },
        {
          type: "response.output_item.added",
          output_index: 0,
          item: { ...item, arguments: "", status: "in_progress" },
        },
        {
          type: "response.function_call_arguments.delta",
          item_id: item.id,
          output_index: 0,
          delta: json,
        },
        {
          type: "response.function_call_arguments.done",
          item_id: item.id,
          output_index: 0,
          arguments: json,
        },
        { type: "response.output_item.done", output_index: 0, item },
        { type: "response.completed", response },
      ].map((value, sequence_number) => ({ ...value, sequence_number }))
    );
  }
  return data({
    candidates: [
      {
        index: 0,
        content: { role: "model", parts: [{ functionCall: { name, args } }] },
        finishReason: "STOP",
      },
    ],
    usageMetadata: { promptTokenCount: 7, candidatesTokenCount: 5, totalTokenCount: 12 },
  });
}

export function errorSse(protocol, partial = false) {
  const initial = frames(protocol).slice(
    0,
    protocol === "anthropic-messages" ? 3 : protocol === "openai-responses" ? 4 : 1
  );
  if (protocol === "google-generative-ai") delete initial[0].candidates[0].finishReason;
  const prefix = partial ? encode(protocol, initial) : "";
  const error = { type: "invalid_request_error", code: "w0_fixture_error", message: errorMarker };
  if (protocol === "anthropic-messages") return prefix + event({ type: "error", error });
  if (protocol === "openai-completions") return prefix + data({ error });
  if (protocol === "openai-responses")
    return (
      prefix +
      event({
        type: "response.failed",
        response: { id: "resp_w0", object: "response", status: "failed", output: [], error },
      })
    );
  return prefix + data({ error: { code: 400, status: "INVALID_ARGUMENT", message: errorMarker } });
}

export function selectedTool(protocol, body) {
  if (!body) return undefined;
  const tools =
    protocol === "google-generative-ai"
      ? body.tools?.flatMap((t) => t.functionDeclarations || [])
      : body.tools;
  return tools
    ?.map((t) => t.name || t.function?.name)
    .find((name) => name?.toLowerCase().replace(/^_/, "") === "read");
}
