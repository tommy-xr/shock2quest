/** Error thrown when the debug runtime returns a non-2xx response. */
export class HttpError extends Error {
  constructor(
    public readonly method: string,
    public readonly url: string,
    public readonly status: number,
    public readonly body: string,
  ) {
    super(`${method} ${url} failed with status ${status}: ${body}`);
    this.name = "HttpError";
  }
}

/** Thin typed wrapper over fetch for the debug runtime's JSON API. */
export class HttpClient {
  constructor(public readonly baseUrl: string) {}

  async get<T>(path: string): Promise<T> {
    return this.request<T>("GET", path);
  }

  async post<T>(path: string, body?: unknown): Promise<T> {
    return this.request<T>("POST", path, body);
  }

  private async request<T>(
    method: string,
    path: string,
    body?: unknown,
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    const response = await fetch(url, {
      method,
      headers: body !== undefined ? { "Content-Type": "application/json" } : undefined,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    const text = await response.text();
    if (!response.ok) {
      throw new HttpError(method, url, response.status, text);
    }
    return JSON.parse(text) as T;
  }
}
