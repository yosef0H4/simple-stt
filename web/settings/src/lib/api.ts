const token = new URLSearchParams(location.hash.slice(1)).get("token") || "";
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
  if (!response.ok) throw Error(data.error || response.statusText);
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
