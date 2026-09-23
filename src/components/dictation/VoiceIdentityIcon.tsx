import { cn } from "../lib/utils";

interface VoiceIdentityIconProps {
  size?: number;
  className?: string;
}

/** The listening ring with its three audio bars. */
export function VoiceIdentityIcon({ size = 24, className }: VoiceIdentityIconProps) {
  return (
    <span
      className={cn("voice-identity-icon relative inline-block shrink-0", className)}
      style={{ width: size, height: size }}
      aria-hidden="true"
    >
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        className="voice-identity-svg absolute inset-0 overflow-visible"
        aria-hidden="true"
      >
        <path
          d="M4.929 19.071C1.024 15.166 1.024 8.834 4.929 4.929C8.834 1.024 15.166 1.024 19.071 4.929C22.976 8.834 22.976 15.166 19.071 19.071C15.166 22.976 8.834 22.976 4.929 19.071Z"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <path
          d="M8.75 10C8.75 11.2 8.75 12.8 8.75 14"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
        />
        <path
          d="M12 8C12 10.2 12 13.8 12 16"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
        />
        <path
          d="M15.25 10C15.25 11.2 15.25 12.8 15.25 14"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
        />
      </svg>
    </span>
  );
}
