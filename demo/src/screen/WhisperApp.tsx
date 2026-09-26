import type { ReactNode } from "react";
import { interpolate, useCurrentFrame } from "remotion";
import { easeOut, hash, ramp } from "../lib/anim.ts";
import { useCopy } from "../lib/copy-context.tsx";
import { revealText } from "../lib/text.ts";
import { APP, MONO } from "../theme.ts";
import { TrafficLights, WhisperGlyph } from "./Desktop.tsx";

export type Page = "home" | "insights" | "upload" | "dictionary" | "settings";

// Sidebar icon outlines, lucide-style like src/components/icons.
const ICONS: Record<Exclude<Page, "settings">, string> = {
  home: "M4 11l8-7 8 7v8a1 1 0 0 1-1 1h-4v-6H9v6H5a1 1 0 0 1-1-1z",
  insights: "M5 20V12M10 20V6M15 20v-9M20 20V4",
  upload: "M12 16V4M7 9l5-5 5 5M4 20h16",
  dictionary: "M4 5a2 2 0 0 1 2-2h5v17H6a2 2 0 0 0-2 2zM20 5a2 2 0 0 0-2-2h-5v17h5a2 2 0 0 1 2 2z",
};

function Icon({ children, size = 24 }: { children: ReactNode; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {children}
    </svg>
  );
}

function CopyIcon() {
  return (
    <Icon size={22}>
      <rect x="9" y="9" width="11" height="11" rx="2" />
      <path d="M5 15V5a1 1 0 0 1 1-1h9" />
    </Icon>
  );
}

interface ShellProps {
  page: Page;
  title: string;
  children: ReactNode;
  /** Page highlighted in the sidebar can lag the content during a switch. */
  x?: number;
  y?: number;
  width?: number;
  height?: number;
}

/** Whisper's main window: sidebar navigation and the rounded content panel. */
export function WhisperShell({
  page,
  title,
  children,
  x = 110,
  y = 56,
  width = 1380,
  height = 800,
}: ShellProps) {
  const copy = useCopy();
  const nav = copy.whisper.nav;
  return (
    <div
      style={{
        position: "absolute",
        left: x,
        top: y,
        width,
        height,
        borderRadius: 16,
        overflow: "hidden",
        background: APP.window,
        color: APP.text,
        fontFamily: copy.font,
        boxShadow: "0 40px 90px rgba(0,0,0,0.45), 0 0 0 1px rgba(255,255,255,0.06)",
        display: "flex",
      }}
    >
      <div
        style={{
          width: 250,
          padding: "18px 12px",
          display: "flex",
          flexDirection: "column",
          gap: 6,
        }}
      >
        <div style={{ padding: "0 8px 30px" }}>
          <TrafficLights />
        </div>
        {(Object.keys(ICONS) as (keyof typeof ICONS)[]).map((id) => (
          <div
            key={id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 14,
              padding: "12px 14px",
              borderRadius: 10,
              fontSize: 21,
              fontWeight: id === page ? 600 : 400,
              background: id === page ? APP.navActive : undefined,
              color: id === page ? "#fff" : "#c9cad0",
            }}
          >
            <span style={{ color: id === page ? APP.primary : "#b5b6bc", display: "flex" }}>
              <Icon>
                <path d={ICONS[id]} />
              </Icon>
            </span>
            {nav[id]}
          </div>
        ))}
        <div style={{ flex: 1 }} />
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 14,
            padding: "12px 14px",
            borderRadius: 10,
            fontSize: 21,
            background: page === "settings" ? APP.navActive : undefined,
          }}
        >
          <Icon>
            <circle cx="12" cy="12" r="3" />
            <path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" />
          </Icon>
          {nav.settings}
        </div>
      </div>
      <div
        style={{
          flex: 1,
          margin: "12px 12px 12px 0",
          borderRadius: 14,
          border: `1px solid ${APP.border}`,
          background: APP.panel,
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          position: "relative",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 18,
            height: 76,
            padding: "0 26px",
            borderBottom: `1px solid ${APP.border}`,
          }}
        >
          <Icon>
            <rect x="3" y="4" width="18" height="16" rx="2" />
            <path d="M9 4v16" />
          </Icon>
          <span style={{ fontSize: 23, fontWeight: 700, minWidth: 200 }}>{title}</span>
          <div
            style={{
              marginLeft: 80,
              width: 480,
              height: 46,
              borderRadius: 23,
              border: `1px solid ${APP.border}`,
              background: APP.surface,
              display: "flex",
              alignItems: "center",
              gap: 12,
              padding: "0 18px",
              color: APP.faint,
              fontSize: 19,
            }}
          >
            <Icon size={20}>
              <circle cx="11" cy="11" r="7" />
              <path d="M20 20l-4-4" />
            </Icon>
            {copy.whisper.search}
            <span
              style={{
                marginLeft: "auto",
                fontSize: 15,
                padding: "3px 10px",
                borderRadius: 10,
                background: APP.raised,
              }}
            >
              ⌘ + K
            </span>
          </div>
        </div>
        <div style={{ flex: 1, position: "relative" }}>{children}</div>
      </div>
    </div>
  );
}

function highlight(text: string, query: string, on: boolean) {
  const index = on ? text.indexOf(query) : -1;
  if (index < 0) return text;
  return (
    <>
      {text.slice(0, index)}
      <mark style={{ background: "rgba(91,134,245,0.35)", color: "#fff", borderRadius: 4 }}>
        {query}
      </mark>
      {text.slice(index + query.length)}
    </>
  );
}

interface HistoryProps {
  /** Command search opens, the query is typed, the match is copied. */
  searchAt: number;
  copyAt: number;
  closeAt: number;
}

/** Home: today's dictations, then ⌘K search and copy. */
export function HistoryPage({ searchAt, copyAt, closeAt }: HistoryProps) {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { whisper } = copy;
  const palette = ramp(frame, searchAt, 8) * (1 - ramp(frame, closeAt, 8));
  const query = revealText(
    whisper.query,
    ramp(frame, searchAt + 8, 12, (t) => t)
  );
  const matches = whisper.history.filter((entry) => entry.text.includes(whisper.query));
  const copied = ramp(frame, copyAt, 8) * (1 - ramp(frame, copyAt + 40, 10));
  const pressed = frame >= copyAt && frame < copyAt + 4;

  return (
    <div style={{ position: "absolute", inset: 0, padding: "26px 30px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 18,
          color: APP.muted,
          fontSize: 18,
          marginBottom: 14,
        }}
      >
        <span style={{ fontWeight: 600 }}>{whisper.today}</span>
        <span style={{ flex: 1, height: 1, background: APP.border }} />
        <span>{whisper.showDiscarded}</span>
        <span>{whisper.clearAll}</span>
      </div>
      {whisper.history.map((entry, i) => {
        const appear = ramp(frame, 4 + i * 3, 10);
        return (
          <div
            key={entry.time}
            style={{
              display: "flex",
              gap: 40,
              padding: "20px 18px",
              borderRadius: 12,
              background: i === 0 ? APP.surface : undefined,
              opacity: appear,
              transform: `translateY(${(1 - appear) * 12}px)`,
            }}
          >
            <span style={{ fontFamily: MONO, color: APP.faint, fontSize: 20, width: 70 }}>
              {entry.time}
            </span>
            <span
              style={{
                flex: 1,
                fontFamily: `${MONO}, ${copy.font}`,
                fontSize: 22,
                lineHeight: 1.5,
              }}
            >
              {entry.text}
            </span>
            {i === 0 && (
              <span style={{ display: "flex", gap: 18, color: APP.muted }}>
                <CopyIcon />
                <span style={{ fontSize: 22 }}>⋮</span>
              </span>
            )}
          </div>
        );
      })}

      {palette > 0 && (
        <div
          style={{ position: "absolute", inset: 0, background: `rgba(0,0,0,${0.45 * palette})` }}
        >
          <div
            style={{
              position: "absolute",
              left: "50%",
              top: 60,
              width: 820,
              transform: `translateX(-50%) scale(${0.96 + 0.04 * palette})`,
              opacity: palette,
              borderRadius: 18,
              background: APP.surface,
              border: `1px solid ${APP.borderHover}`,
              boxShadow: "0 30px 60px rgba(0,0,0,0.45)",
              overflow: "hidden",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 14,
                padding: "22px 24px",
                borderBottom: `1px solid ${APP.border}`,
                fontSize: 24,
              }}
            >
              <Icon>
                <circle cx="11" cy="11" r="7" />
                <path d="M20 20l-4-4" />
              </Icon>
              {query}
              <span style={{ width: 2, height: 28, background: APP.primary }} />
            </div>
            <div style={{ padding: "12px 12px 16px" }}>
              <div style={{ fontSize: 16, color: APP.faint, padding: "6px 14px" }}>
                {copy.lang === "en" ? "Transcripts" : "转录"}
              </div>
              {query.length === copy.whisper.query.length &&
                matches.map((entry, i) => (
                  <div
                    key={entry.time}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 22,
                      padding: "16px 14px",
                      borderRadius: 12,
                      background: i === 0 ? APP.navActive : undefined,
                      fontSize: 21,
                    }}
                  >
                    <span style={{ fontFamily: MONO, color: APP.faint, fontSize: 17 }}>
                      {entry.time}
                    </span>
                    <span style={{ flex: 1, fontFamily: `${MONO}, ${copy.font}` }}>
                      {highlight(entry.text, whisper.query, true)}
                    </span>
                    {i === 0 && (
                      <span
                        style={{
                          color: copied > 0 ? APP.success : APP.muted,
                          transform: `scale(${pressed ? 0.85 : 1})`,
                          display: "flex",
                        }}
                      >
                        <CopyIcon />
                      </span>
                    )}
                  </div>
                ))}
            </div>
          </div>
        </div>
      )}
      {copied > 0 && (
        <div
          style={{
            position: "absolute",
            right: 30,
            bottom: 30,
            padding: "16px 22px",
            borderRadius: 14,
            background: APP.raised,
            border: `1px solid ${APP.borderHover}`,
            fontSize: 20,
            opacity: copied,
            transform: `translateY(${(1 - copied) * 20}px)`,
          }}
        >
          <div style={{ fontWeight: 700 }}>{whisper.copied}</div>
          <div style={{ color: APP.muted, fontSize: 17, marginTop: 4 }}>
            {copy.lang === "en" ? "Text copied to your clipboard" : "文本已复制到剪贴板"}
          </div>
        </div>
      )}
    </div>
  );
}

function FileIcon({ name }: { name: string }) {
  const video = name.endsWith(".mp4");
  return (
    <span
      style={{
        width: 46,
        height: 56,
        borderRadius: 8,
        background: video ? "#8a6fd1" : "#2a9d8f",
        display: "inline-flex",
        alignItems: "flex-end",
        justifyContent: "center",
        paddingBottom: 6,
        fontSize: 12,
        fontWeight: 700,
        color: "#fff",
        flexShrink: 0,
      }}
    >
      {video ? "MP4" : "M4A"}
    </span>
  );
}

interface UploadProps {
  dropAt: number;
  progressAt: number;
  doneAt: number;
}

/** Upload: files dropped on the zone transcribe one after another. */
export function UploadPage({ dropAt, progressAt, doneAt }: UploadProps) {
  const frame = useCurrentFrame();
  const { whisper } = useCopy();
  const { upload } = whisper;
  const dropped = frame >= dropAt + 10;
  const span = (doneAt - progressAt) / upload.files.length;
  const progressOf = (i: number) => ramp(frame, progressAt + i * span * 0.8, span * 1.2, (t) => t);
  const done = upload.files.filter((_, i) => progressOf(i) >= 1).length;
  const complete = ramp(frame, doneAt, 10);

  return (
    <div style={{ position: "absolute", inset: 0, padding: "34px 60px" }}>
      <div style={{ fontSize: 30, fontWeight: 700, marginBottom: 22 }}>{upload.title}</div>
      {!dropped ? (
        <div
          style={{
            height: 380,
            borderRadius: 20,
            border: `2px dashed ${frame >= dropAt ? APP.primary : APP.borderHover}`,
            background: frame >= dropAt ? "rgba(91,134,245,0.08)" : APP.surface,
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 16,
            color: APP.muted,
          }}
        >
          <Icon size={54}>
            <path d={ICONS.upload} />
          </Icon>
          <div style={{ fontSize: 24, color: APP.text }}>{upload.drop}</div>
          <div style={{ fontSize: 18 }}>{upload.formats}</div>
        </div>
      ) : (
        <div
          style={{
            borderRadius: 20,
            border: `1px solid ${APP.border}`,
            background: APP.surface,
            padding: "10px 0",
          }}
        >
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              padding: "12px 28px",
              color: APP.muted,
              fontSize: 19,
            }}
          >
            <span>{upload.progress(done, upload.files.length)}</span>
            {complete > 0 && (
              <span style={{ color: APP.success, opacity: complete }}>
                ✓ {upload.complete} · {upload.saved}
              </span>
            )}
          </div>
          {upload.files.map((name, i) => {
            const progress = progressOf(i);
            const appear = ramp(frame, dropAt + 10 + i * 3, 10);
            return (
              <div
                key={name}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 20,
                  padding: "16px 28px",
                  opacity: appear,
                  transform: `translateX(${(1 - appear) * 40}px)`,
                }}
              >
                <FileIcon name={name} />
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 22, marginBottom: 10 }}>{name}</div>
                  <div
                    style={{
                      height: 8,
                      borderRadius: 4,
                      background: APP.raised,
                      overflow: "hidden",
                    }}
                  >
                    <div
                      style={{
                        width: `${progress * 100}%`,
                        height: "100%",
                        background: progress >= 1 ? APP.success : APP.primary,
                      }}
                    />
                  </div>
                </div>
                <span
                  style={{
                    width: 40,
                    textAlign: "right",
                    color: progress >= 1 ? APP.success : APP.muted,
                    fontSize: 22,
                  }}
                >
                  {progress >= 1 ? "✓" : `${Math.round(progress * 100)}%`}
                </span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** Files flying in from the right edge of the screen onto the drop zone. */
export function DraggedFiles({ dropAt }: { dropAt: number }) {
  const frame = useCurrentFrame();
  const { whisper } = useCopy();
  const t = ramp(frame, dropAt - 20, 20, easeOut);
  const vanish = ramp(frame, dropAt + 4, 6);
  if (t <= 0 || vanish >= 1) return null;
  return (
    <div style={{ position: "absolute", inset: 0, pointerEvents: "none" }}>
      {whisper.upload.files.map((name, i) => (
        <div
          key={name}
          style={{
            position: "absolute",
            left: interpolate(t, [0, 1], [1640, 760 + i * 36]),
            top: interpolate(t, [0, 1], [640 - i * 40, 380 + i * 18]),
            transform: `rotate(${(i - 1) * 8}deg)`,
            opacity: 1 - vanish,
            filter: "drop-shadow(0 12px 20px rgba(0,0,0,0.35))",
          }}
        >
          <FileIcon name={name} />
        </div>
      ))}
    </div>
  );
}

interface InsightsProps {
  countAt: number;
}

/** Insights: the day's totals count up and the activity grid lights. */
export function InsightsPage({ countAt }: InsightsProps) {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { insights } = copy.whisper;
  const t = ramp(frame, countAt, 40, easeOut);
  const format = (value: number) => Math.round(value * t).toLocaleString(copy.lang);
  const cards = [
    { label: insights.words, value: format(insights.values.words), note: insights.allTime },
    {
      label: insights.streak,
      value: insights.days(Math.round(insights.values.streak * t)),
      note: "",
    },
    { label: insights.wpm, value: format(insights.values.wpm), note: "" },
    { label: insights.dictations, value: format(insights.values.dictations), note: "" },
  ];
  const weeks = 26;
  return (
    <div style={{ position: "absolute", inset: 0, padding: "30px 44px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 16, marginBottom: 24 }}>
        <span style={{ fontSize: 28, fontWeight: 700 }}>{insights.title}</span>
        <span
          style={{
            fontSize: 16,
            padding: "5px 12px",
            borderRadius: 12,
            background: APP.raised,
            color: APP.muted,
          }}
        >
          {insights.onDevice}
        </span>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 18 }}>
        {cards.map((card) => (
          <div
            key={card.label}
            style={{
              padding: "22px 24px",
              borderRadius: 16,
              background: APP.surface,
              border: `1px solid ${APP.border}`,
            }}
          >
            <div style={{ fontSize: 18, color: APP.muted }}>{card.label}</div>
            <div
              style={{
                fontSize: 44,
                fontWeight: 700,
                marginTop: 10,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {card.value}
            </div>
          </div>
        ))}
      </div>
      <div
        style={{
          marginTop: 26,
          padding: "22px 24px",
          borderRadius: 16,
          background: APP.surface,
          border: `1px solid ${APP.border}`,
        }}
      >
        <div style={{ fontSize: 18, color: APP.muted, marginBottom: 16 }}>{insights.activity}</div>
        <div style={{ display: "grid", gridTemplateColumns: `repeat(${weeks}, 1fr)`, gap: 6 }}>
          {Array.from({ length: weeks * 7 }, (_, i) => {
            const week = Math.floor(i / 7);
            const level = hash(i + 3) * (0.4 + (week / weeks) * 0.6);
            const lit = ramp(frame, countAt + week * 1.2, 8);
            const today = i === weeks * 7 - 3;
            return (
              <span
                key={i}
                style={{
                  gridColumn: week + 1,
                  gridRow: (i % 7) + 1,
                  height: 26,
                  borderRadius: 5,
                  background: today ? APP.primary : `rgba(91,134,245,${0.08 + level * 0.8 * lit})`,
                  boxShadow: today ? `0 0 ${14 * lit}px rgba(91,134,245,0.9)` : undefined,
                }}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}

interface SettingsProps {
  asrAt: number;
  cleanupAt: number;
}

function Field({ label, value, at }: { label: string; value: string; at: number }) {
  const frame = useCurrentFrame();
  const typed = revealText(
    value,
    ramp(frame, at, 14, (t) => t)
  );
  const focused = frame >= at && frame < at + 22;
  return (
    <div style={{ marginBottom: 18 }}>
      <div style={{ fontSize: 18, color: APP.muted, marginBottom: 8 }}>{label}</div>
      <div
        style={{
          height: 52,
          borderRadius: 12,
          border: `1.5px solid ${focused ? APP.primary : APP.border}`,
          background: APP.window,
          display: "flex",
          alignItems: "center",
          padding: "0 18px",
          fontFamily: MONO,
          fontSize: 21,
        }}
      >
        {typed}
        {focused && (
          <span style={{ width: 2, height: 24, background: APP.primary, marginLeft: 2 }} />
        )}
      </div>
    </div>
  );
}

/** Settings: the speech and cleanup servers the user runs. */
export function SettingsPage({ asrAt, cleanupAt }: SettingsProps) {
  const frame = useCurrentFrame();
  const copy = useCopy();
  const { settings } = copy.whisper;
  const toggle = ramp(frame, cleanupAt - 6, 6);
  return (
    <div style={{ position: "absolute", inset: 0, display: "flex" }}>
      <div
        style={{
          width: 240,
          borderRight: `1px solid ${APP.border}`,
          padding: "24px 14px",
          fontSize: 19,
          color: APP.muted,
        }}
      >
        {[
          settings.speechToText,
          settings.cleanup,
          copy.whisper.nav.dictionary,
          copy.lang === "en" ? "Hotkeys" : "快捷键",
        ].map((item, i) => (
          <div
            key={item}
            style={{
              padding: "11px 14px",
              borderRadius: 10,
              background: i < 2 ? APP.navActive : undefined,
              color: i < 2 ? "#fff" : undefined,
              marginBottom: 4,
            }}
          >
            {item}
          </div>
        ))}
      </div>
      <div style={{ flex: 1, padding: "26px 40px", overflow: "hidden" }}>
        <div
          style={{ display: "flex", alignItems: "center", gap: 12, fontSize: 26, fontWeight: 700 }}
        >
          <WhisperGlyph size={26} color={APP.primary} />
          {settings.speechToText}
        </div>
        <div style={{ fontSize: 17, color: APP.muted, margin: "6px 0 20px" }}>
          {settings.shared}
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "1.4fr 1fr", gap: 20 }}>
          <Field label={settings.endpoint} value={settings.asrUrl} at={asrAt} />
          <Field label={settings.model} value={settings.asrModel} at={asrAt + 8} />
        </div>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            margin: "12px 0 16px",
          }}
        >
          <span style={{ fontSize: 26, fontWeight: 700 }}>{settings.cleanup}</span>
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: 12,
              fontSize: 18,
              color: APP.muted,
            }}
          >
            {settings.enableCleanup}
            <span
              style={{
                width: 52,
                height: 30,
                borderRadius: 15,
                background: toggle > 0.5 ? APP.primary : APP.raised,
                position: "relative",
              }}
            >
              <span
                style={{
                  position: "absolute",
                  top: 3,
                  left: 3 + toggle * 22,
                  width: 24,
                  height: 24,
                  borderRadius: 12,
                  background: "#fff",
                }}
              />
            </span>
          </span>
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "1.4fr 1fr", gap: 20 }}>
          <Field label={settings.endpoint} value={settings.cleanupUrl} at={cleanupAt} />
          <Field label={settings.model} value={settings.cleanupModel} at={cleanupAt + 8} />
        </div>
      </div>
    </div>
  );
}
