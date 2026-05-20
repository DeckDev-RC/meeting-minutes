import "../templates/minutes.css";

interface Props {
  html: string;
}

export default function MinutesPreview({ html }: Props) {
  return (
    <div
      id="minutes-preview"
      className="minutes-wrapper"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
