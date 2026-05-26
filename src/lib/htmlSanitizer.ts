import DOMPurify from "dompurify";

export function sanitizeMinutesHtml(html: string): string {
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["script", "style", "iframe", "object", "embed"],
    FORBID_ATTR: ["srcdoc"],
    ALLOW_DATA_ATTR: false,
  });
}

