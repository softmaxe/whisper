import { useCurrentFrame } from "remotion";
import { ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { revealText } from "../lib/text.ts";
import { Window } from "./Desktop.tsx";

/** Local frames at which the body text starts and finishes landing. */
export function Mail({ pasteAt }: { pasteAt: number }) {
  const frame = useCurrentFrame();
  const { mail } = useCopy();
  const typed = revealText(
    mail.cleaned,
    ramp(frame, pasteAt, 8, (t) => t)
  );
  const glow = ramp(frame, pasteAt, 6) * (1 - ramp(frame, pasteAt + 18, 16));
  const caretOn = Math.floor(frame / 15) % 2 === 0;
  const field = (label: string, value: string) => (
    <div
      style={{
        display: "flex",
        gap: 14,
        padding: "16px 34px",
        borderBottom: "1px solid #ececf0",
        fontSize: 22,
      }}
    >
      <span style={{ color: "#8a8a92", minWidth: 90 }}>{label}</span>
      <span style={{ fontWeight: 500 }}>{value}</span>
    </div>
  );
  return (
    <Window x={250} y={80} width={1100} height={690} title={mail.subjectText}>
      <div
        style={{
          display: "flex",
          gap: 12,
          padding: "12px 24px",
          borderBottom: "1px solid #ececf0",
        }}
      >
        <span
          style={{
            padding: "8px 20px",
            borderRadius: 10,
            background: "#5b86f5",
            color: "#fff",
            fontSize: 18,
            fontWeight: 600,
          }}
        >
          ➤
        </span>
        {[0, 1, 2].map((i) => (
          <span key={i} style={{ width: 40, height: 36, borderRadius: 8, background: "#f0f0f4" }} />
        ))}
      </div>
      {field(mail.to, mail.toName)}
      {field(mail.subject, mail.subjectText)}
      <div style={{ padding: "30px 34px", fontSize: 27, lineHeight: 1.6 }}>
        <div>{mail.greeting}</div>
        <div
          style={{
            marginTop: 14,
            borderRadius: 10,
            background: `rgba(91,134,245,${0.14 * glow})`,
            boxShadow: `0 0 0 ${8 * glow}px rgba(91,134,245,${0.1 * glow})`,
            minHeight: 44,
          }}
        >
          {typed}
          {caretOn && (
            <span
              style={{
                display: "inline-block",
                width: 2.5,
                height: 32,
                background: "#1d1d1f",
                verticalAlign: -6,
                marginLeft: 2,
              }}
            />
          )}
        </div>
      </div>
    </Window>
  );
}
