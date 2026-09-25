import { useCurrentFrame } from "remotion";
import { LIGHT, type Light, type TimeOfDay } from "../theme.ts";

export type Place = "kitchen" | "office" | "cafe" | "living" | "bedroom";

interface SkyProps {
  x: number;
  y: number;
  width: number;
  height: number;
  time: TimeOfDay;
  light: Light;
  id: string;
}

/** What shows through a window: gradient sky, sun or moon, clouds or stars. */
function Sky({ x, y, width, height, time, light, id }: SkyProps) {
  const frame = useCurrentFrame();
  const drift = (frame * 0.4) % width;
  const sun: Partial<Record<TimeOfDay, [number, number, number, string]>> = {
    dawn: [0.3, 0.86, 70, "#fff1c9"],
    morning: [0.78, 0.25, 48, "#fff7d6"],
    noon: [0.6, 0.12, 52, "#fffbe6"],
    afternoon: [0.72, 0.55, 58, "#fff0b8"],
    evening: [0.42, 0.95, 80, "#ffd08a"],
  };
  const disc = sun[time];
  const night = time === "night";
  return (
    <g>
      <defs>
        <linearGradient id={`sky-${id}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={light.skyTop} />
          <stop offset="1" stopColor={light.skyBottom} />
        </linearGradient>
        <clipPath id={`clip-${id}`}>
          <rect x={x} y={y} width={width} height={height} rx="10" />
        </clipPath>
      </defs>
      <g clipPath={`url(#clip-${id})`}>
        <rect x={x} y={y} width={width} height={height} fill={`url(#sky-${id})`} />
        {disc && (
          <>
            <circle
              cx={x + width * disc[0]}
              cy={y + height * disc[1]}
              r={disc[2] * 1.8}
              fill={disc[3]}
              opacity="0.25"
            />
            <circle cx={x + width * disc[0]} cy={y + height * disc[1]} r={disc[2]} fill={disc[3]} />
          </>
        )}
        {night && (
          <>
            {Array.from({ length: 22 }, (_, i) => {
              const sx = x + ((i * 97) % width);
              const sy = y + ((i * 53) % (height * 0.7));
              const twinkle = 0.45 + 0.55 * Math.abs(Math.sin(frame / 14 + i));
              return (
                <circle
                  key={i}
                  cx={sx}
                  cy={sy}
                  r={i % 3 === 0 ? 3 : 2}
                  fill="#fff"
                  opacity={twinkle}
                />
              );
            })}
            <circle cx={x + width * 0.72} cy={y + height * 0.28} r="46" fill="#fff6d8" />
            <circle
              cx={x + width * 0.72 + 20}
              cy={y + height * 0.28 - 12}
              r="42"
              fill={light.skyTop}
            />
          </>
        )}
        {!night &&
          [0, 1].map((i) => {
            const cx = x + ((i * width * 0.55 + drift + 60) % (width + 200)) - 100;
            const cy = y + height * (0.3 + i * 0.22);
            return (
              <g key={i} fill="#fff" opacity={time === "evening" ? 0.35 : 0.85}>
                <ellipse cx={cx} cy={cy} rx="70" ry="22" />
                <ellipse cx={cx - 30} cy={cy - 14} rx="34" ry="24" />
                <ellipse cx={cx + 22} cy={cy - 18} rx="40" ry="28" />
              </g>
            );
          })}
      </g>
    </g>
  );
}

function WindowFrame({
  x,
  y,
  width,
  height,
  color,
}: {
  x: number;
  y: number;
  width: number;
  height: number;
  color: string;
}) {
  return (
    <g>
      <rect x={x - 16} y={y - 16} width={width + 32} height={height + 32} rx="16" fill={color} />
      <rect x={x} y={y} width={width} height={height} rx="10" fill="none" />
      <rect x={x - 26} y={y + height + 8} width={width + 52} height="18" rx="9" fill={color} />
    </g>
  );
}

function Mullions({
  x,
  y,
  width,
  height,
  color,
}: {
  x: number;
  y: number;
  width: number;
  height: number;
  color: string;
}) {
  return (
    <g fill={color}>
      <rect x={x + width / 2 - 6} y={y} width="12" height={height} />
      <rect x={x} y={y + height / 2 - 6} width={width} height="12" />
    </g>
  );
}

function Plant({ x, y, scale = 1 }: { x: number; y: number; scale?: number }) {
  const frame = useCurrentFrame();
  const sway = Math.sin(frame / 30) * 2;
  return (
    <g transform={`translate(${x} ${y}) scale(${scale})`}>
      <g transform={`rotate(${sway} 0 0)`} fill="#3f9b6e">
        <ellipse cx="-38" cy="-120" rx="26" ry="70" transform="rotate(-28 -38 -120)" />
        <ellipse cx="36" cy="-128" rx="26" ry="74" transform="rotate(24 36 -128)" fill="#4fb07e" />
        <ellipse cx="0" cy="-160" rx="24" ry="80" fill="#57b986" />
        <ellipse cx="-62" cy="-70" rx="20" ry="52" transform="rotate(-55 -62 -70)" fill="#4fb07e" />
        <ellipse cx="60" cy="-72" rx="20" ry="52" transform="rotate(55 60 -72)" />
      </g>
      <path d="M-58 -40 H58 L46 60 H-46 Z" fill="#e9825b" />
      <rect x="-64" y="-50" width="128" height="18" rx="6" fill="#d9704a" />
    </g>
  );
}

function Mug({ x, y, color, steam }: { x: number; y: number; color: string; steam?: boolean }) {
  const frame = useCurrentFrame();
  return (
    <g transform={`translate(${x} ${y})`}>
      {steam &&
        [0, 1, 2].map((i) => {
          const t = ((frame + i * 20) % 60) / 60;
          return (
            <path
              key={i}
              d={`M${-14 + i * 14} ${-40 - t * 50} q 10 -14 0 -28 q -10 -14 0 -28`}
              stroke="#fff"
              strokeWidth="5"
              fill="none"
              strokeLinecap="round"
              opacity={0.6 * Math.sin(t * Math.PI)}
            />
          );
        })}
      <rect x="-34" y="-40" width="68" height="78" rx="14" fill={color} />
      <path d="M34 -20 q 30 0 30 22 q 0 22 -30 22" stroke={color} strokeWidth="10" fill="none" />
      <ellipse cx="0" cy="-40" rx="34" ry="8" fill="#6b4431" />
    </g>
  );
}

function Lamp({ x, y, on }: { x: number; y: number; on: boolean }) {
  return (
    <g transform={`translate(${x} ${y})`}>
      {on && <path d="M-40 -150 L-230 120 H230 L40 -150 Z" fill="#ffd98a" opacity="0.18" />}
      <rect x="-8" y="-160" width="16" height="160" fill="#3a3550" />
      <path d="M-70 -150 L-40 -230 H40 L70 -150 Z" fill={on ? "#ffcf6e" : "#d7a54a"} />
      <ellipse cx="0" cy="0" rx="60" ry="12" fill="#3a3550" />
    </g>
  );
}

function Books({ x, y }: { x: number; y: number }) {
  const colors = ["#e76f51", "#2a9d8f", "#e9c46a", "#8a6fd1", "#f4a261"];
  return (
    <g transform={`translate(${x} ${y})`}>
      {colors.map((color, i) => (
        <rect
          key={color}
          x={i * 30}
          y={-120 + (i % 2) * 14}
          width="26"
          height={120 - (i % 2) * 14}
          rx="3"
          fill={color}
        />
      ))}
      <rect
        x="160"
        y="-110"
        width="26"
        height="112"
        rx="3"
        fill="#264653"
        transform="rotate(14 173 0)"
      />
    </g>
  );
}

function Cat({ x, y }: { x: number; y: number }) {
  const frame = useCurrentFrame();
  const breathe = 1 + Math.sin(frame / 18) * 0.03;
  return (
    <g transform={`translate(${x} ${y})`}>
      <g transform={`scale(1 ${breathe})`}>
        <ellipse cx="0" cy="-34" rx="92" ry="44" fill="#f2a65a" />
        <ellipse cx="-70" cy="-50" rx="42" ry="38" fill="#f2a65a" />
        <path d="M-100 -76 l8 -34 l20 26 Z M-58 -84 l14 -30 l12 30 Z" fill="#f2a65a" />
        <path
          d="M-88 -52 q 8 6 16 0 M-62 -52 q 8 6 16 0"
          stroke="#5b3a29"
          strokeWidth="4"
          fill="none"
          strokeLinecap="round"
        />
        <path
          d="M80 -20 q 60 0 40 -50"
          stroke="#f2a65a"
          strokeWidth="18"
          fill="none"
          strokeLinecap="round"
        />
        <path
          d="M-20 -70 q 20 -6 40 0 M10 -64 q 20 -6 40 0"
          stroke="#e08a3e"
          strokeWidth="6"
          fill="none"
          strokeLinecap="round"
        />
      </g>
    </g>
  );
}

/** The room behind the desk. Drawn in the 1920×1080 world. */
export function Backdrop({ place, time }: { place: Place; time: TimeOfDay }) {
  const light = LIGHT[time];
  const frame = "#fdf8f1";
  return (
    <svg
      width="1920"
      height="1080"
      viewBox="0 0 1920 1080"
      style={{ position: "absolute", inset: 0 }}
    >
      <rect width="1920" height="1080" fill={light.wall} />
      {place === "kitchen" && (
        <>
          <WindowFrame x={150} y={110} width={520} height={400} color={frame} />
          <Sky x={150} y={110} width={520} height={400} time={time} light={light} id="kitchen" />
          <path
            d="M150 510 q 80 -70 160 -30 q 70 -80 170 -20 q 90 -60 190 10 V510Z"
            fill="#7cae7a"
            opacity="0.9"
          />
          <Mullions x={150} y={110} width={520} height={400} color={frame} />
          <rect x="1460" y="330" width="360" height="16" rx="6" fill={light.wallShade} />
          {["#f4a261", "#e9c46a", "#2a9d8f"].map((color, i) => (
            <g key={color} transform={`translate(${1500 + i * 110} 330)`}>
              <rect x="-30" y="-90" width="60" height="90" rx="12" fill={color} opacity="0.9" />
              <rect x="-34" y="-104" width="68" height="18" rx="6" fill="#c7866a" />
            </g>
          ))}
          <path d="M1640 0 V120" stroke="#3a3550" strokeWidth="6" />
          <path d="M1580 170 q 60 -70 120 0 Z" fill="#e9825b" />
        </>
      )}
      {place === "office" && (
        <>
          <WindowFrame x={120} y={90} width={640} height={440} color="#f5f7fa" />
          <Sky x={120} y={90} width={640} height={440} time={time} light={light} id="office" />
          <g fill="#8aa1c1" opacity="0.8">
            <rect x="140" y="330" width="80" height="200" />
            <rect x="230" y="270" width="110" height="260" />
            <rect x="350" y="360" width="70" height="170" />
            <rect x="430" y="300" width="120" height="230" />
            <rect x="560" y="380" width="90" height="150" />
            <rect x="660" y="320" width="90" height="210" />
          </g>
          <Mullions x={120} y={90} width={640} height={440} color="#f5f7fa" />
          <circle cx="1640" cy="220" r="74" fill="#fff" stroke={light.wallShade} strokeWidth="10" />
          <path
            d="M1640 220 V172 M1640 220 L1674 238"
            stroke="#3a3550"
            strokeWidth="7"
            strokeLinecap="round"
          />
          <Plant x={1740} y={640} scale={1.25} />
          {time === "afternoon" && (
            <path d="M120 90 L760 90 L1320 1080 L420 1080 Z" fill="#fff3c4" opacity="0.18" />
          )}
        </>
      )}
      {place === "cafe" && (
        <>
          <rect width="1920" height="1080" fill="#d9b48f" />
          <g fill="#caa27c">
            {Array.from({ length: 14 }, (_, row) =>
              Array.from({ length: 12 }, (_, col) => (
                <rect
                  key={`${row}-${col}`}
                  x={col * 170 + (row % 2) * 85 - 40}
                  y={260 + row * 48}
                  width="150"
                  height="36"
                  rx="4"
                />
              ))
            )}
          </g>
          <WindowFrame x={180} y={260} width={560} height={320} color="#2f5d50" />
          <Sky x={180} y={260} width={560} height={320} time={time} light={light} id="cafe" />
          <g fill="#6aa66e">
            <circle cx="300" cy="560" r="80" />
            <circle cx="420" cy="540" r="100" />
            <circle cx="620" cy="570" r="90" />
          </g>
          <Mullions x={180} y={260} width={560} height={320} color="#2f5d50" />
          <g>
            {Array.from({ length: 12 }, (_, i) => (
              <path
                key={i}
                d={`M${i * 170 - 20} 0 H${i * 170 + 150} V170 q -85 50 -170 0 Z`}
                fill={i % 2 ? "#fff6ea" : "#e45d4c"}
              />
            ))}
          </g>
          <Plant x={1560} y={660} scale={1.1} />
          <Plant x={1790} y={660} scale={0.9} />
        </>
      )}
      {place === "living" && (
        <>
          <WindowFrame x={140} y={110} width={600} height={420} color="#efe6ef" />
          <Sky x={140} y={110} width={600} height={420} time={time} light={light} id="living" />
          <path
            d="M140 530 V420 l60 -30 l40 40 l70 -70 l80 60 l60 -40 l90 70 l100 -50 l60 60 V530 Z"
            fill="#4b3f6b"
            opacity="0.75"
          />
          <Mullions x={140} y={110} width={600} height={420} color="#efe6ef" />
          <rect x="1440" y="120" width="300" height="220" rx="10" fill="#fff" opacity="0.9" />
          <path d="M1470 310 l70 -90 l50 60 l40 -40 l80 70 Z" fill="#e59a7a" />
          <circle cx="1680" cy="180" r="22" fill="#ffd08a" />
          <rect x="1420" y="520" width="480" height="140" rx="40" fill="#7b6aa8" />
          <Cat x={1640} y={540} />
        </>
      )}
      {place === "bedroom" && (
        <>
          <WindowFrame x={140} y={100} width={540} height={420} color="#3f4478" />
          <Sky x={140} y={100} width={540} height={420} time={time} light={light} id="bedroom" />
          <Mullions x={140} y={100} width={540} height={420} color="#3f4478" />
          <rect x="1440" y="300" width="380" height="16" rx="6" fill={light.wallShade} />
          <Books x={1480} y={300} />
          <g opacity="0.9">
            {[0, 1, 2, 3, 4, 5].map((i) => (
              <circle key={i} cx={900 + i * 150} cy={70 + (i % 2) * 20} r="9" fill="#ffd98a" />
            ))}
            <path
              d="M840 60 q 150 60 300 20 q 150 -40 300 20 q 100 30 200 10"
              stroke="#8c8fb8"
              strokeWidth="3"
              fill="none"
            />
          </g>
        </>
      )}
    </svg>
  );
}

/** Things standing on the desk around the laptop. */
export function DeskProps({ place, time }: { place: Place; time: TimeOfDay }) {
  return (
    <svg
      width="1920"
      height="1080"
      viewBox="0 0 1920 1080"
      style={{ position: "absolute", inset: 0 }}
    >
      {place === "kitchen" && <Mug x={1560} y={720} color="#f4efe6" steam />}
      {place === "office" && (
        <>
          <Mug x={1580} y={720} color="#5b86f5" steam={time !== "afternoon"} />
          <rect
            x="360"
            y="650"
            width="210"
            height="60"
            rx="8"
            fill="#f7f1e3"
            transform="rotate(-6 465 680)"
          />
        </>
      )}
      {place === "cafe" && (
        <g transform="translate(1560 720)">
          <ellipse cx="0" cy="40" rx="90" ry="18" fill="#fff" />
          <rect x="-44" y="-30" width="88" height="70" rx="20" fill="#fff" />
          <ellipse cx="0" cy="-30" rx="44" ry="10" fill="#c98f62" />
          <path d="M-12 -32 q 12 -10 24 0 q -12 8 -24 0" fill="#f7e7d6" />
        </g>
      )}
      {place === "living" && <Mug x={1520} y={730} color="#e9c46a" />}
      {place === "bedroom" && <Lamp x={1580} y={700} on={time === "night"} />}
    </svg>
  );
}
