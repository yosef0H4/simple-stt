const token = new URLSearchParams(location.hash.slice(1)).get("token") || "";
export class ApiError extends Error {
  message_id?: string;
  message_args?: Record<string, string | number>;
  raw?: string;
  constructor(message: string, detail?: { message_id?: string; message_args?: unknown }) {
    super(message);
    this.name = "ApiError";
    this.message_id = detail?.message_id;
    if (typeof detail?.message_args === "string") {
      try { this.message_args = JSON.parse(detail.message_args); } catch { this.message_args = {}; }
    } else if (detail?.message_args && typeof detail.message_args === "object") {
      this.message_args = detail.message_args as Record<string, string | number>;
    }
    this.raw = message;
  }
}
export async function api<T>(
  path: string,
  body?: unknown,
  signal?: AbortSignal,
): Promise<T> {
  const response = await fetch(path, {
    method: body === undefined ? "GET" : "POST",
    headers: {
      "X-Simple-STT-Token": token,
      ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal,
  });
  const data = await response.json();
  if (!response.ok) throw new ApiError(data.error || response.statusText, data);
  return data as T;
}
export async function events(
  after: number,
  signal: AbortSignal,
): Promise<import("./types").ServiceEvent[]> {
  const response = await fetch(`/api/events?after=${after}`, {
    headers: { "X-Simple-STT-Token": token },
    signal,
  });
  if (!response.ok) throw Error("Event connection failed");
  const line = (await response.text())
    .split("\n")
    .find((v) => v.startsWith("data: "));
  return line ? JSON.parse(line.slice(6)).events : [];
}
