import { forwardRef, type HTMLAttributes } from "react";
import { ChevronUp } from "../icons";
import { cn } from "../lib/utils";
import { FlowWaveform } from "./FlowWaveform";
import { PillWaveform } from "./PillWaveform";
import { VoiceIdentityIcon } from "./VoiceIdentityIcon";
import { RESTING_WAVE_SILHOUETTE, WAVEFORM_BAR_COUNT } from "./waveformMath";
import {
  resolveVoicePillShape,
  VOICE_PILL_FOOTPRINT,
  VOICE_PILL_GROW_TRANSITION,
} from "../../helpers/voicePillPresentation";

export type VoicePillState =
  "idle" | "hover" | "recording" | "processing" | "thinking" | "unavailable";

interface VoicePillProps extends Omit<HTMLAttributes<HTMLDivElement>, "children"> {
  variant: "floating" | "panel";
  state: VoicePillState;
  getAudioLevel: () => number | null;
  expanded?: boolean;
  collapseToLogo?: boolean;
  waveformVisible?: boolean;
  waveformOnlyWhileRecording?: boolean;
  integratedWithPanel?: boolean;
  /** The cancel button's liquid skin owns the fused surface; go headless. */
  liquidFused?: boolean;
  showExpandChevron?: boolean;
  isDragging?: boolean;
  horizontalDirection?: "left" | "right";
}

// Sized from WAVEFORM_BAR_COUNT so a bar-count change can never silently
// desync the resting silhouette from the live waveform's footprint.
const RESTING_WAVE_HEIGHTS = Array.from(
  { length: WAVEFORM_BAR_COUNT },
  (_, index) => RESTING_WAVE_SILHOUETTE[index % RESTING_WAVE_SILHOUETTE.length]
);

// Icon↔waveform spacing inside the compact panel pill. Together with the 98px
// panel footprint and pr-1.5, centering lands the documented 6/12 edge insets
// (VOICE_PILL_FOOTPRINT in voicePillPresentation.js).
const COMPACT_CONTENT_GAP_PX = 6;

// Mirrored by .liquid-cancel-skin[data-pill-state] in dictation-panel.css,
// which redraws this chrome while the cancel skin owns the fused surface —
// change together. The listening Flow bar skips these for its always-black
// .voice-pill-control[data-flow-bar] chrome.
const STATE_APPEARANCE: Record<VoicePillState, string> = {
  idle: "border-border-hover bg-surface-1 text-muted-foreground dark:border-border/50",
  hover: "border-border-hover bg-surface-3 text-foreground",
  recording: "border-border-hover bg-surface-1 text-foreground",
  processing: "border-border/60 bg-surface-1 text-foreground/70",
  thinking: "border-border/60 bg-surface-1 text-foreground",
  unavailable: "border-border/60 bg-surface-1 text-muted-foreground",
};

/** One persistent control that resizes between the floating and panel layouts. */
export const VoicePill = forwardRef<HTMLDivElement, VoicePillProps>(function VoicePill(
  {
    variant,
    state,
    getAudioLevel,
    expanded = false,
    collapseToLogo = false,
    waveformVisible = true,
    waveformOnlyWhileRecording = false,
    integratedWithPanel = false,
    liquidFused = false,
    showExpandChevron = false,
    isDragging = false,
    horizontalDirection = "right",
    className,
    style,
    ...props
  },
  ref
) {
  const isRecording = state === "recording";
  const isProcessing = state === "processing";
  const isThinking = state === "thinking";
  const isUnavailable = state === "unavailable";
  // The Signal glow (comet orbit over a breathing halo) lights for the real
  // thinking state alone — glowing during the entrance or while listening would read
  // as work already in flight before any transcript exists.
  const showSignalGlow = !isUnavailable && isThinking;
  const isPanel = variant === "panel";
  const shape = resolveVoicePillShape({
    variant,
    state,
    expanded,
    collapseToLogo,
    waveformOnlyWhileRecording,
  });
  const showCompactPill = shape !== "idle";
  // The floating pill listens as the icon-less Flow bar; the panel pill keeps
  // its identity beside the scrolling waveform.
  const flowBar = shape === "listening";
  const panelCompact = shape === "panel";
  const showDivider = panelCompact && waveformVisible && !isRecording;
  // The hidden divider's margins are what carry the compact pill's 6px
  // icon↔waveform gap; a visible divider keeps 4px flanking its 1px rule.
  const dividerMargin = panelCompact ? (showDivider ? 4 : COMPACT_CONTENT_GAP_PX / 2) : 0;
  const identitySize = 22;
  const floatingHover = !isPanel && state === "hover";
  const footprint = VOICE_PILL_FOOTPRINT[shape];

  const pill = (
    <div
      ref={ref}
      className={cn(
        "voice-pill-control relative flex items-center justify-center overflow-hidden rounded-full border",
        panelCompact && "pr-1.5",
        "shadow-[var(--shadow-card)]",
        !flowBar && STATE_APPEARANCE[state],
        className
      )}
      style={{
        // Listening uses the compact pill. A wide recording bar would make the
        // control feel like a different surface and force a large window resize.
        width: footprint.width,
        height: footprint.height,
        cursor: isProcessing || isThinking ? "not-allowed" : isDragging ? "grabbing" : "pointer",
        // Yields to the fused rule's `box-shadow: none` while the liquid skin
        // owns the chrome — an inline shadow would outrank it and paint a
        // phantom capsule when a de-fusing skin lingers over a hovered pill.
        boxShadow: floatingHover && !liquidFused ? "var(--shadow-card-hover-subtle)" : undefined,
        transition: `width ${VOICE_PILL_GROW_TRANSITION}, height ${VOICE_PILL_GROW_TRANSITION}, padding-left ${VOICE_PILL_GROW_TRANSITION}, padding-right ${VOICE_PILL_GROW_TRANSITION}, background-color 220ms ease-out, border-color 220ms ease-out, box-shadow 220ms ease-out`,
        ...style,
      }}
      data-horizontal-direction={horizontalDirection}
      data-flow-bar={flowBar || undefined}
      data-integrated-with-panel={integratedWithPanel || undefined}
      data-liquid-fused={liquidFused || undefined}
      data-expand-chevron={showExpandChevron || undefined}
      {...props}
    >
      <div
        className="pointer-events-none absolute inset-0 bg-gradient-to-br from-foreground/10 to-transparent transition-opacity duration-200 ease-out"
        style={{ opacity: state === "hover" ? 0.72 : 0 }}
      />

      <span
        className="voice-pill-identity-slot relative inline-block shrink-0"
        style={{
          width: flowBar ? 0 : identitySize,
          height: identitySize,
          opacity: flowBar ? 0 : 1,
          transition: `width ${VOICE_PILL_GROW_TRANSITION}, opacity 180ms ease-out`,
        }}
        aria-hidden="true"
      >
        <span
          className={cn(
            "voice-pill-identity-logo absolute inset-0 transition-[opacity,transform] duration-200 ease-out",
            showExpandChevron ? "translate-y-1 scale-75 opacity-0" : "scale-100 opacity-100"
          )}
        >
          <VoiceIdentityIcon
            size={identitySize}
            className={cn(
              "transition-[width,height] duration-200",
              state === "idle" && "text-foreground",
              (isUnavailable || isProcessing) && "animate-pulse"
            )}
          />
        </span>
        <ChevronUp
          className={cn(
            "voice-pill-expand-chevron absolute inset-0 m-auto size-5 transition-[opacity,transform] duration-200 ease-out",
            showExpandChevron ? "scale-100 opacity-100" : "translate-y-1 scale-75 opacity-0"
          )}
          strokeWidth={2}
        />
      </span>

      <div
        className="shrink-0 overflow-hidden bg-border/60"
        style={{
          height: showCompactPill ? 16 : 20,
          width: showDivider ? 1 : 0,
          marginLeft: dividerMargin,
          marginRight: dividerMargin,
          opacity: showDivider ? 1 : 0,
          transition: `width ${VOICE_PILL_GROW_TRANSITION}, margin ${VOICE_PILL_GROW_TRANSITION}, opacity 180ms ease-out`,
        }}
      />

      <div
        className={cn(
          "voice-pill-waveform relative shrink-0 overflow-hidden",
          isPanel && "text-foreground"
        )}
        style={{
          width: showCompactPill ? 52 : 0,
          height: showCompactPill ? 24 : 32,
          transition: `width ${VOICE_PILL_GROW_TRANSITION}, height ${VOICE_PILL_GROW_TRANSITION}`,
        }}
      >
        {!isPanel ? (
          // Kept mounted while the pill collapses so the bars fade with it
          // instead of vanishing mid-shrink.
          <FlowWaveform
            getLevel={getAudioLevel}
            active={flowBar && isRecording}
            resting={isUnavailable}
            className={cn("absolute inset-0", isUnavailable && "animate-pulse")}
            style={{
              opacity: flowBar && waveformVisible && !showExpandChevron ? 1 : 0,
              transition: "opacity 200ms ease-out",
            }}
          />
        ) : (
          <>
            <div
              className="absolute inset-0 flex items-center justify-center gap-0.75 transition-opacity duration-200 ease-out"
              style={{ opacity: showCompactPill && waveformVisible && !isRecording ? 1 : 0 }}
              aria-hidden="true"
            >
              {RESTING_WAVE_HEIGHTS.map((height, index) => (
                <span
                  key={`${height}-${index}`}
                  className="w-0.5 rounded-full bg-current"
                  style={{ height }}
                />
              ))}
            </div>
            <PillWaveform
              getLevel={getAudioLevel}
              active={isRecording}
              className={cn(
                "absolute inset-0 transition-opacity duration-200 ease-out",
                showCompactPill && waveformVisible && isRecording ? "opacity-100" : "opacity-0"
              )}
            />
          </>
        )}
      </div>

      {!isPanel && (
        <ChevronUp
          className={cn(
            "voice-flow-chevron pointer-events-none absolute inset-0 m-auto size-5 transition-[opacity,transform] duration-200 ease-out",
            flowBar && showExpandChevron
              ? "scale-100 opacity-100"
              : "translate-y-1 scale-75 opacity-0"
          )}
          strokeWidth={2}
        />
      )}

      {isUnavailable && (
        <div
          className={cn(
            "pointer-events-none absolute inset-0 rounded-full border-2 animate-pulse",
            flowBar ? "border-white/30" : "border-foreground/30"
          )}
        />
      )}
    </div>
  );

  return (
    <span className="voice-pill-glow-anchor">
      <span
        aria-hidden="true"
        className="processing-signal-glow"
        data-active={showSignalGlow ? "true" : undefined}
      >
        <span className="processing-signal-ring" />
      </span>
      {pill}
    </span>
  );
});
