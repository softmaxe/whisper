// Split query/hash so path-suffix stripping and later joins operate on the
// path only. Provider docs and Azure/gateway pastes often include ?api-version=
// (or similar) on a full /chat/completions URL (#1309).
function splitUrlDecorators(value: string): { path: string; query: string; hash: string } {
  let path = value;
  let hash = "";
  const hashIndex = path.indexOf("#");
  if (hashIndex >= 0) {
    hash = path.slice(hashIndex);
    path = path.slice(0, hashIndex);
  }
  let query = "";
  const queryIndex = path.indexOf("?");
  if (queryIndex >= 0) {
    query = path.slice(queryIndex);
    path = path.slice(0, queryIndex);
  }
  return { path, query, hash };
}

function joinUrlDecorators(path: string, query: string, hash: string): string {
  return `${path}${query}${hash}`;
}

// API Configuration helpers
export const normalizeBaseUrl = (value?: string | null): string => {
  if (!value) return "";

  const trimmed = value.trim();
  if (!trimmed) return "";

  const { path: rawPath, query, hash } = splitUrlDecorators(trimmed);
  let normalized = rawPath;

  // Remove common API endpoint suffixes to get the base URL
  const suffixReplacements: Array<[RegExp, string]> = [
    [/\/v1\/chat\/completions$/i, "/v1"],
    [/\/chat\/completions$/i, ""],
    [/\/v1\/responses$/i, "/v1"],
    [/\/responses$/i, ""],
    [/\/v1\/models$/i, "/v1"],
    [/\/models$/i, ""],
    [/\/v1\/audio\/transcriptions$/i, "/v1"],
    [/\/audio\/transcriptions$/i, ""],
    [/\/v1\/audio\/translations$/i, "/v1"],
    [/\/audio\/translations$/i, ""],
  ];

  for (const [pattern, replacement] of suffixReplacements) {
    if (pattern.test(normalized)) {
      normalized = normalized.replace(pattern, replacement).replace(/\/+$/, "");
    }
  }

  return joinUrlDecorators(normalized.replace(/\/+$/, ""), query, hash);
};

export const buildApiUrl = (base: string, path: string): string => {
  const normalizedBase = normalizeBaseUrl(base) || "https://api.openai.com/v1";
  if (!path) {
    return normalizedBase;
  }
  const normalizedPath = path.startsWith("/") ? path : `/${path}`;
  const { path: originAndPath, query, hash } = splitUrlDecorators(normalizedBase);
  return joinUrlDecorators(`${originAndPath}${normalizedPath}`, query, hash);
};

export const ensureV1Suffix = (base: string): string => {
  if (!base) return base;
  const normalized = normalizeBaseUrl(base) || base;
  const { path, query, hash } = splitUrlDecorators(normalized);
  return joinUrlDecorators(path.endsWith("/v1") ? path : `${path}/v1`, query, hash);
};

// Ordered bases to try when listing models from an OpenAI-compatible server.
// Self-hosted servers (LM Studio, Ollama, vLLM) serve the API under /v1 even
// when users enter the bare origin, and LM Studio's native REST base
// (/api/v1 or /api/v0) has its OpenAI-compatible sibling at /v1.
export const getModelListBaseCandidates = (base: string): string[] => {
  const normalized = normalizeBaseUrl(base);
  if (!normalized) return [];
  const { path, query, hash } = splitUrlDecorators(normalized);
  const nativeApiMatch = path.match(/^(.+?)\/api\/v[01]$/i);
  if (nativeApiMatch) {
    return [normalized, joinUrlDecorators(`${nativeApiMatch[1]}/v1`, query, hash)];
  }
  const withV1 = ensureV1Suffix(normalized);
  return withV1 === normalized ? [normalized] : [normalized, withV1];
};

const env = (typeof import.meta !== "undefined" && (import.meta as any).env) || {};

const computeBaseUrl = (candidates: Array<string | undefined>, fallback: string): string => {
  for (const candidate of candidates) {
    const normalized = normalizeBaseUrl(candidate);
    if (normalized) {
      return normalized;
    }
  }
  return fallback;
};

const DEFAULT_OPENAI_BASE = computeBaseUrl(
  [env.OPENWHISPR_OPENAI_BASE_URL as string | undefined, env.OPENAI_BASE_URL as string | undefined],
  "https://api.openai.com/v1"
);

const DEFAULT_TRANSCRIPTION_BASE = computeBaseUrl(
  [
    env.OPENWHISPR_TRANSCRIPTION_BASE_URL as string | undefined,
    env.WHISPER_BASE_URL as string | undefined,
  ],
  DEFAULT_OPENAI_BASE
);

// List length above which pickers switch to a searchable variant.
export const LIST_SEARCH_THRESHOLD = 12;

// Token Limits
export const TOKEN_LIMITS = {
  MIN_TOKENS: 512,
  MAX_TOKENS: 2048,
  MIN_TOKENS_ANTHROPIC: 100,
  MAX_TOKENS_ANTHROPIC: 4096,
  TOKEN_MULTIPLIER: 2, // text.length * multiplier
  REASONING_CONTEXT_SIZE: 4096,
} as const;

// Retry Configuration
export const RETRY_CONFIG = {
  MAX_RETRIES: 3,
  INITIAL_DELAY: 1000, // 1 second
  MAX_DELAY: 10000, // 10 seconds
  BACKOFF_MULTIPLIER: 2,
} as const;
