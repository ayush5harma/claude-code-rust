/** Bound and redact SDK stderr before it enters startup diagnostics or the wire. */
export function startupFailureDetails(errors: unknown): string[] {
  if (!Array.isArray(errors) || !errors.every((entry) => typeof entry === "string")) {
    return [];
  }
  return errors.slice(0, 8).map((error: string) =>
    redactStartupDetail(error).slice(0, 1_024),
  );
}

/**
 * Twin of `redact_text` in `src/cli/redaction.rs`, which is the reference: its
 * table test lists the shapes both must cover, and a rule added to one is added
 * to the other.
 */
export function redactStartupDetail(value: string): string {
  return value
    .replace(/\bBearer\s+[^\s"',;]+/gi, "Bearer [redacted]")
    .replace(/\bsk-[a-z0-9_-]+/gi, "[redacted]")
    .replace(/(https?:\/\/)[^\s/@]+@/gi, "$1[redacted]@")
    .replace(
      /((?:api[_-]?key|token|secret|password|credential|authorization)[a-z0-9_-]*["']?\s*[:=]\s*)(?:(?:basic|digest|negotiate|bearer)\s+)?(?:"[^"\n]*"|'[^'\n]*'|[^\s,;&]+)/gi,
      "$1[redacted]",
    );
}
