import { useCurrentFrame } from "remotion";
import { lerp } from "../lib/anim.ts";
import { PERSON } from "../theme.ts";

interface PersonProps {
  /** 0..1, how strongly the speech arcs show. */
  speaking: number;
  /** 0..1, how far the left hand is pressing the fn key. */
  press: number;
  /** 0..1, arms raised in a stretch. */
  stretch?: number;
  night?: boolean;
}

const HEAD = { x: 700, y: 600 };

/** Arms reach from the shoulders to the keyboard; drawn behind the torso. */
export function Arms({ press, stretch = 0 }: Pick<PersonProps, "press" | "stretch">) {
  const frame = useCurrentFrame();
  const typing = Math.sin(frame / 3.2) * 3;
  const left = {
    x: lerp(930, 560, stretch),
    y: lerp(704 + press * 8, 330, stretch),
  };
  const right = {
    x: lerp(1150, 860, stretch),
    y: lerp(702 + typing * (1 - stretch), 320, stretch),
  };
  const sleeve = (fromX: number, fromY: number, to: { x: number; y: number }, bend: number) =>
    `M${fromX} ${fromY} Q ${(fromX + to.x) / 2 + bend} ${(fromY + to.y) / 2 + 90 * (1 - stretch)} ${to.x} ${to.y}`;
  return (
    <svg
      width="1920"
      height="1080"
      viewBox="0 0 1920 1080"
      style={{ position: "absolute", inset: 0 }}
    >
      <path
        d={sleeve(540, 860, left, 60)}
        stroke={PERSON.sweaterShade}
        strokeWidth="64"
        fill="none"
        strokeLinecap="round"
      />
      <path
        d={sleeve(870, 860, right, -40)}
        stroke={PERSON.sweater}
        strokeWidth="64"
        fill="none"
        strokeLinecap="round"
      />
      <ellipse cx={left.x} cy={left.y} rx="26" ry="18" fill={PERSON.skin} />
      <ellipse cx={right.x} cy={right.y} rx="26" ry="18" fill={PERSON.skin} />
    </svg>
  );
}

/** The person at the desk, seen over the shoulder. */
export function Person({ speaking, stretch = 0, night }: PersonProps) {
  const frame = useCurrentFrame();
  const breathe = Math.sin(frame / 22) * 4;
  const nod = speaking * Math.sin(frame / 4.5) * 2.5;
  const lean = stretch * -10;
  return (
    <svg
      width="1920"
      height="1080"
      viewBox="0 0 1920 1080"
      style={{ position: "absolute", inset: 0 }}
    >
      <g transform={`translate(0 ${breathe}) rotate(${lean} 700 1000)`}>
        {/* Torso */}
        <path
          d="M430 1090 C 440 900 500 830 600 805 L 800 805 C 900 830 960 900 970 1090 Z"
          fill={PERSON.sweater}
        />
        <path
          d="M430 1090 C 440 900 500 830 600 805 L 640 805 C 560 860 520 960 520 1090 Z"
          fill={PERSON.sweaterShade}
        />
        <path d="M630 790 q 70 40 140 0 v 24 q -70 34 -140 0 Z" fill={PERSON.collar} />
        {/* Neck */}
        <rect x="660" y="690" width="80" height="115" rx="30" fill={PERSON.skinShade} />
        {/* Head */}
        <g transform={`rotate(${nod} ${HEAD.x} ${HEAD.y + 90})`}>
          <ellipse cx={HEAD.x + 98} cy={HEAD.y + 18} rx="18" ry="28" fill={PERSON.skin} />
          <ellipse cx={HEAD.x - 98} cy={HEAD.y + 18} rx="16" ry="26" fill={PERSON.skinShade} />
          <ellipse cx={HEAD.x} cy={HEAD.y} rx="104" ry="116" fill={PERSON.hair} />
          <path
            d={`M${HEAD.x - 104} ${HEAD.y + 10} q 10 90 70 104 q 34 8 68 0 q 60 -14 70 -104`}
            fill={PERSON.hair}
          />
          <path
            d={`M${HEAD.x - 60} ${HEAD.y - 70} q 50 -40 120 -8`}
            stroke={PERSON.hairLight}
            strokeWidth="14"
            fill="none"
            strokeLinecap="round"
          />
          <circle cx={HEAD.x + 10} cy={HEAD.y - 104} r="40" fill={PERSON.hair} />
          <circle cx={HEAD.x + 22} cy={HEAD.y - 116} r="14" fill={PERSON.hairLight} opacity="0.6" />
          {night && (
            <path
              d={`M${HEAD.x - 112} ${HEAD.y + 10} q 0 -140 112 -140 q 112 0 112 140`}
              stroke="#e45d4c"
              strokeWidth="16"
              fill="none"
            />
          )}
        </g>
        {/* Speech arcs */}
        <g
          transform={`translate(${HEAD.x + 150} ${HEAD.y - 20})`}
          stroke="#fff"
          strokeWidth="9"
          fill="none"
          strokeLinecap="round"
          opacity={speaking}
        >
          {[0, 1, 2].map((i) => {
            const pulse = (frame / 10 + i / 3) % 1;
            return (
              <path
                key={i}
                d={`M${i * 26} -${24 + i * 14} q ${18 + i * 8} ${24 + i * 14} 0 ${48 + i * 28}`}
                opacity={1 - pulse * 0.8}
              />
            );
          })}
        </g>
      </g>
    </svg>
  );
}
