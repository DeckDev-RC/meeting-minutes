import "../templates/minutes.css";
import { useMemo } from "react";
import { sanitizeMinutesHtml } from "../lib/htmlSanitizer";

interface Props {
  html: string;
}

export default function MinutesPreview({ html }: Props) {
  const safeHtml = useMemo(() => sanitizeMinutesHtml(html), [html]);

  return (
    <div
      id="minutes-preview"
      className="minutes-wrapper"
      dangerouslySetInnerHTML={{ __html: safeHtml }}
    />
  );
}
