import { AbsoluteFill } from "remotion";
import { BEAT5_DURATIONS, BEAT5_TEXT, beat5 as beat } from "../../../timeline/beats/beat5-servers.ts";
import { ramp, useBeatTime } from "../anim.ts";
import { RecordingPill } from "../chrome/RecordingPill.tsx";
import { isBlinking, Whis, whisPose } from "../characters/Whis.tsx";
import { HAND_FONT } from "../fonts.ts";
import { useLang } from "../lang.tsx";
import { useCopy } from "../../lib/copy-context.tsx";
import { PenArrow, PenNote } from "../pen/RedPen.tsx";
import { speechLevel } from "../pill/waveform.ts";
import { RoughDrawing, useWobbleSeed } from "../rough/RoughDrawing.tsx";
import { PALETTE } from "../theme.ts";
import { type DataPath, HouseOutline, type HouseShape, Mac, MONO, pointOn, Server, Written } from "./beat5/House.tsx";
import { Logo } from "./beat5/Logo.tsx";

/* Composition (canvas px). Captions are written below y = 850. */
const GROUND_Y = 832;
const WHIS = { x: 180, scale: 0.85 };
const HOUSE: HouseShape = { left: 380, right: 1540, eave: 300, base: 800, apex: 64, overhang: 44 };
const MAC = { x: 450, y: 350, w: 400, h: 260 };
const SERVER = { x: 930, w: 540, h: 118 };
const STT_Y = 340;
const CLEANUP_Y = 580;
/** Endpoints are written just under their server. */
const ENDPOINT_GAP = 44;
/** The data arrows: Mac → speech-to-text → cleanup → Mac. All stay inside the house walls. */
const DATA: DataPath[] = [
  { from: [MAC.x + MAC.w + 12, 420], to: [SERVER.x - 14, STT_Y + 50], bend: 24 },
  { from: [SERVER.x + SERVER.w + 6, STT_Y + 70], to: [SERVER.x + SERVER.w + 6, CLEANUP_Y + 48], bend: 46 },
  { from: [SERVER.x - 14, CLEANUP_Y + 60], to: [MAC.x + MAC.w + 12, 540], bend: 24 },
];
/** Data packets circulating round the three arrows. */
const PACKETS = 5;
const LAP_SECONDS = 2.4;
/** The Dictation on the Mac closes this long after it stops. */
const PILL_DONE_AFTER = 0.6;
/** Ending layout: the logo, the install command card and the repository link. */
const LOGO = { cx: 960, cy: 250, size: 340 };
const CARD = { x: 520, y: 470, w: 880, h: 96 };
const LINK = { x: 960, y: 668 };

/**
 * Beat 5 · Your servers. The house outline is drawn with the Mac and the two
 * servers inside, each server named and labelled with its local endpoint.
 * Pencil data arrows join them and red data packets circulate along them,
 * never crossing the walls, while a Dictation runs on the Mac; the red pen
 * notes in the roof that the data never leaves the house. The house lifts
 * off, the Whisper logo is drawn with its W in one stroke, the `brew
 * install` command and the repository link are written, and Whis waves.
 * The Beat fades itself out before the Film's last second, so that second
 * is blank paper (under whatever wash the Film lays over the paper).
 */
export const Beat5Servers: React.FC = () => {
  const { frame, fps, t } = useBeatTime(beat);
  const lang = useLang();
  const copy = useCopy();
  const m = beat.moments;
  const d = BEAT5_DURATIONS;
  const settings = copy.whisper.settings;

  const beatOpacity = 1 - ramp(t, m.beatOut, m.beatOut + d.beatOut);
  const houseOut = ramp(t, m.houseOut, m.houseOut + d.houseOut);

  // House scene reveals.
  const houseP = ramp(t, m.sketchIn, m.sketchIn + 1.3);
  const macP = ramp(t, m.mac, m.mac + 0.8);
  const serverP = m.servers.map((at) => ramp(t, at, at + 0.7));
  const labelP = m.labels.map((at) => ramp(t, at, at + 0.6));
  const arrowP = m.arrows.map((at) => ramp(t, at, at + 0.45));
  const homeP = ramp(t, m.home, m.home + 0.9);
  const flowing = t >= m.listen && t < m.houseOut + d.houseOut;
  const flowIn = ramp(t, m.listen, m.listen + 0.3);

  // Ending reveals.
  const squareP = ramp(t, m.logo, m.logo + 0.6);
  const strokeP = ramp(t, m.logoStroke, m.logoStroke + d.logoStroke);
  const cardP = ramp(t, m.install, m.install + 0.35);
  const commandP = ramp(t, m.install + 0.25, m.install + 1.05);
  const linkP = ramp(t, m.link, m.link + 0.6);

  const packets = flowing ? packetPositions(t - m.listen) : [];
  const serverLight = (i: number) => (flowing ? 0.5 + 0.5 * Math.sin((t - m.listen) * 9 + i * 2) : 0);

  return (
    <AbsoluteFill style={{ opacity: beatOpacity }}>
      <svg width={1920} height={1080} style={{ position: "absolute", inset: 0 }}>
        {houseOut < 1 && (
          <g opacity={1 - houseOut} transform={`translate(0 ${-40 * houseOut})`}>
            <HouseOutline house={HOUSE} progress={houseP} seed={501} />
            <Mac {...MAC} progress={macP} seed={511}>
              <RecordingPill
                cx={MAC.x + MAC.w / 2}
                cy={MAC.y + MAC.h / 2}
                scale={1.5}
                t={t}
                listenAt={m.listen}
                stopAt={m.stop}
                doneAt={m.stop + PILL_DONE_AFTER}
                seed={561}
              />
              <TextLanded progress={ramp(t, m.stop + PILL_DONE_AFTER, m.stop + PILL_DONE_AFTER + 0.7)} />
            </Mac>
            <Written
              x={MAC.x + MAC.w / 2}
              y={MAC.y + MAC.h + 140}
              anchor="middle"
              progress={ramp(t, m.mac + 0.6, m.mac + 1.1)}
              fontSize={44}
              font={`"${HAND_FONT}", serif`}
              color={PALETTE.graphite}
            >
              {BEAT5_TEXT.mac[lang]}
            </Written>
            {[
              { y: STT_Y, name: settings.speechToText, url: settings.asrUrl },
              { y: CLEANUP_Y, name: settings.cleanup, url: settings.cleanupUrl },
            ].map((s, i) => (
              <g key={i}>
                <Server
                  x={SERVER.x}
                  y={s.y}
                  w={SERVER.w}
                  h={SERVER.h}
                  name={s.name}
                  progress={serverP[i]}
                  nameProgress={labelP[i]}
                  light={serverLight(i)}
                  seed={521 + i * 10}
                />
                <EndpointTag x={SERVER.x} y={s.y + SERVER.h + ENDPOINT_GAP} url={s.url} progress={labelP[i]} seed={541 + i} />
              </g>
            ))}
            {DATA.map((a, i) => (
              <PenArrow
                key={i}
                from={a.from}
                to={a.to}
                bend={a.bend}
                progress={arrowP[i]}
                seed={551 + i}
                options={{ stroke: PALETTE.graphite, strokeWidth: 4 }}
              />
            ))}
            <g opacity={flowIn}>
              {packets.map(([x, y], i) => (
                <circle key={i} cx={x} cy={y} r={9} fill={PALETTE.redPen} stroke={PALETTE.paper} strokeWidth={3} />
              ))}
            </g>
            <PenNote
              x={(HOUSE.left + HOUSE.right) / 2}
              y={HOUSE.eave - 44}
              text={BEAT5_TEXT.home[lang]}
              progress={homeP}
              fontSize={52}
              align="middle"
            />
          </g>
        )}

        <Logo cx={LOGO.cx} cy={LOGO.cy} size={LOGO.size} square={squareP} stroke={strokeP} seed={571} />
        <CommandCard progress={cardP} textProgress={commandP} command={copy.outro.install} />
        <PenNote x={LINK.x} y={LINK.y} text={copy.outro.link} progress={linkP} fontSize={60} align="middle" rotate={-1.5} />

        <Whis
          x={WHIS.x}
          y={GROUND_Y}
          scale={WHIS.scale}
          seed={23}
          draw={ramp(t, m.sketchIn + 0.3, m.sketchIn + 1.2)}
          {...whisAt()}
        />
      </svg>
    </AbsoluteFill>
  );

  /** Whis's pose: watching the house go up, speaking the Dictation, proud of the house, then waving goodbye. */
  function whisAt() {
    const idle = whisPose("idle", frame, fps);
    if (t < m.listen) return { ...idle, look: [1, -0.25] as const };
    if (t < m.stop) {
      const level = speechLevel(frame + Math.round(beat.start * fps));
      return {
        ...idle,
        eyes: isBlinking(frame, fps) ? ("closed" as const) : ("open" as const),
        look: [1, -0.3] as const,
        squash: 1 + level * 0.35,
        rightArm: 8 + 60 * level,
      };
    }
    if (t < m.home) return { ...idle, look: [1, -0.25] as const };
    if (t < m.houseOut) return whisPose("proud", frame, fps);
    if (t < m.wave) return { ...idle, look: [1, -0.5] as const };
    return { ...whisPose("wave", frame, fps), look: [0.2, -0.1] as const };
  }
};

/**
 * Positions of the data packets `s` seconds after they start: they leave the
 * Mac one after another, evenly spaced, one lap of all arrows per LAP_SECONDS.
 */
function packetPositions(s: number) {
  return Array.from({ length: PACKETS }, (_, k) => s / LAP_SECONDS - k / PACKETS)
    .filter((laps) => laps >= 0)
    .map((laps) => {
      const along = (laps % 1) * DATA.length;
      const i = Math.min(DATA.length - 1, Math.floor(along));
      return pointOn(DATA[i], along - i);
    });
}

/** The cleaned text landing on the Mac's screen once the Dictation is done: pencilled lines, written in turn. */
const TextLanded: React.FC<{ progress: number }> = ({ progress }) => {
  const boil = useWobbleSeed(591, 4, progress < 1);
  if (progress <= 0) return null;
  const x = MAC.x + 50;
  const widths = [0.78, 0.84, 0.46].map((f) => f * (MAC.w - 100));
  return (
    <RoughDrawing
      seed={boil}
      options={{ stroke: PALETTE.ink, strokeWidth: 5, roughness: 1.4, bowing: 1.5 }}
      progress={progress}
      build={(g, o) => widths.map((w, i) => g.line(x, MAC.y + 80 + i * 50, x + w, MAC.y + 80 + i * 50, o))}
    />
  );
};

/** A local endpoint URL in a pencilled tag, written after the server's name. */
const EndpointTag: React.FC<{ x: number; y: number; url: string; progress: number; seed: number }> = ({
  x,
  y,
  url,
  progress,
  seed,
}) => {
  const boil = useWobbleSeed(seed, 4, progress < 1);
  if (progress <= 0) return null;
  const fontSize = 26;
  const w = url.length * fontSize * 0.6 + 36;
  return (
    <g>
      <RoughDrawing
        seed={boil}
        options={{ stroke: PALETTE.pencil, strokeWidth: 2.2, roughness: 1.2 }}
        progress={Math.min(1, progress * 2)}
        deps={[x, y, w]}
        build={(g, o) => [g.rectangle(x, y - fontSize - 6, w, fontSize + 22, o)]}
      />
      <Written x={x + 18} y={y + 3} progress={progress} fontSize={fontSize} font={MONO} color={PALETTE.ink}>
        {url}
      </Written>
    </g>
  );
};

/** The install command in a hand-drawn terminal card: `$` prompt and the command written left to right. */
const CommandCard: React.FC<{ progress: number; textProgress: number; command: string }> = ({
  progress,
  textProgress,
  command,
}) => {
  const boil = useWobbleSeed(581, 4, progress < 1 || textProgress < 1);
  if (progress <= 0) return null;
  const { x, y, w, h } = CARD;
  const fontSize = 32;
  return (
    <g>
      <rect x={x} y={y} width={w} height={h} fill={PALETTE.whitePaper} opacity={progress} />
      <RoughDrawing
        seed={boil}
        options={{ stroke: PALETTE.graphite, strokeWidth: 3.2, roughness: 1.1 }}
        progress={progress}
        build={(g, o) => [g.rectangle(x, y, w, h, o)]}
      />
      <text x={x + 30} y={y + h / 2 + 11} fontFamily={MONO} fontSize={fontSize} fill={PALETTE.redPen} opacity={progress}>
        $
      </text>
      <Written x={x + 70} y={y + h / 2 + 11} progress={textProgress} fontSize={fontSize} font={MONO} color={PALETTE.ink}>
        {command}
      </Written>
    </g>
  );
};
