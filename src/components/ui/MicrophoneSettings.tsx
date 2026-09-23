import React, { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { SettingsRow } from "./SettingsSection";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./select";
import { Button } from "./button";
import { RefreshCw, Mic } from "../icons";
import { isBuiltInMicrophone } from "../../utils/audioDeviceUtils";
import { resolveMicrophoneSelection } from "../../helpers/microphoneSelection";
import { resolveMicDeviceSelection } from "../../helpers/micDeviceSelection";
import type { MicrophoneSelectionMode } from "../../stores/settingsStore";

interface AudioDevice {
  kind: "audioinput";
  deviceId: string;
  label: string;
  isBuiltIn: boolean;
}

interface MicrophoneSettingsProps {
  microphoneSelectionMode: MicrophoneSelectionMode;
  selectedMicDeviceId: string;
  selectedMicDeviceLabel: string;
  onSelectionModeChange: (mode: MicrophoneSelectionMode) => void;
  onDeviceSelect: (deviceId: string, label: string) => void;
}

export const MicrophoneSettings: React.FC<MicrophoneSettingsProps> = ({
  microphoneSelectionMode,
  selectedMicDeviceId,
  selectedMicDeviceLabel,
  onSelectionModeChange,
  onDeviceSelect,
}) => {
  const { t } = useTranslation();
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [systemDefault, setSystemDefault] = useState<Awaited<
    ReturnType<Window["electronAPI"]["getSystemDefaultMicrophone"]>
  > | null>(null);
  const [lidClosed, setLidClosed] = useState<boolean | null>(null);
  const mounted = useRef(false);
  const deviceRequest = useRef(0);
  const lidRevision = useRef(0);

  const loadDevices = useCallback(async () => {
    const request = ++deviceRequest.current;
    const isCurrent = () => mounted.current && request === deviceRequest.current;
    setIsLoading(true);
    setError(null);

    try {
      // Acquiring the mic just to read labels interrupts other audio (pauses
      // music on macOS), so only do it when labels are missing (no permission yet).
      let allDevices = await navigator.mediaDevices.enumerateDevices();
      if (!isCurrent()) return;
      const hasLabels = allDevices.some((d) => d.kind === "audioinput" && d.label);
      if (!hasLabels) {
        const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        stream.getTracks().forEach((track) => track.stop());
        if (!isCurrent()) return;
        allDevices = await navigator.mediaDevices.enumerateDevices();
      }
      if (!isCurrent()) return;

      const audioInputs = allDevices
        .filter((d) => d.kind === "audioinput")
        .map((d) => ({
          kind: "audioinput" as const,
          deviceId: d.deviceId,
          label: d.label || `Microphone ${d.deviceId.slice(0, 8)}`,
          isBuiltIn: isBuiltInMicrophone(d.label),
        }));

      setDevices(audioInputs);
      const resolvedSelection = resolveMicDeviceSelection(
        audioInputs,
        selectedMicDeviceId,
        selectedMicDeviceLabel
      );
      if (
        microphoneSelectionMode === "specific" &&
        resolvedSelection.device &&
        resolvedSelection.status === "exact" &&
        !selectedMicDeviceLabel
      ) {
        onDeviceSelect(resolvedSelection.device.deviceId, resolvedSelection.device.label);
      }
      const nativeDefault = await window.electronAPI
        ?.getSystemDefaultMicrophone?.()
        .catch(() => null);
      if (isCurrent()) setSystemDefault(nativeDefault || null);
    } catch {
      if (isCurrent()) setError(t("microphoneSettings.errors.unableToAccess"));
    } finally {
      if (isCurrent()) setIsLoading(false);
    }
  }, [microphoneSelectionMode, onDeviceSelect, selectedMicDeviceId, selectedMicDeviceLabel, t]);

  useEffect(() => {
    mounted.current = true;
    loadDevices();

    const handleDeviceChange = () => loadDevices();
    navigator.mediaDevices.addEventListener("devicechange", handleDeviceChange);

    return () => {
      mounted.current = false;
      deviceRequest.current += 1;
      navigator.mediaDevices.removeEventListener("devicechange", handleDeviceChange);
    };
  }, [loadDevices]);

  useEffect(() => {
    let active = true;
    const revision = lidRevision.current;
    const unsubscribe = window.electronAPI?.onLaptopLidStateChanged?.((closed) => {
      lidRevision.current += 1;
      if (active) setLidClosed(closed);
    });
    window.electronAPI
      ?.getLaptopLidState?.()
      .then((closed) => {
        if (active && revision === lidRevision.current) setLidClosed(closed);
      })
      .catch(() => {});
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  const physicalDevices = devices.filter(
    (device) => !["default", "communications"].includes(device.deviceId)
  );
  const isAuto = microphoneSelectionMode === "auto" || microphoneSelectionMode === "system";
  const builtInDevice = physicalDevices.find((device) => device.isBuiltIn);
  const selectedDevice =
    microphoneSelectionMode === "built-in"
      ? builtInDevice
      : physicalDevices.find((device) => device.deviceId === selectedMicDeviceId);
  const selection = resolveMicrophoneSelection(
    devices,
    {
      microphoneSelectionMode: isAuto ? "auto" : microphoneSelectionMode,
      selectedMicDeviceId,
      selectedMicDeviceLabel,
    },
    systemDefault,
    lidClosed
  );
  const resolvedLabel =
    selection.device?.label || systemDefault?.name || t("microphoneSettings.systemDefault");
  const unavailableLabel = t("microphoneSettings.unavailableDevice", {
    device:
      microphoneSelectionMode === "built-in"
        ? t("microphoneSettings.builtInDevice")
        : selectedMicDeviceLabel || t("microphoneSettings.unknownDevice"),
  });
  const selectorValue = isAuto ? "__auto__" : selectedDevice?.deviceId || "__unavailable__";

  return (
    <div className="space-y-4">
      <div className="space-y-3">
        <div className="flex items-center gap-2">
          <label className="text-sm font-medium text-foreground">
            {t("microphoneSettings.inputDevice")}
          </label>
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("microphoneSettings.refreshDevices")}
            onClick={loadDevices}
            disabled={isLoading}
            className="size-7"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isLoading ? "animate-spin" : ""}`} />
          </Button>
        </div>

        {error ? (
          <p className="text-sm text-destructive">{error}</p>
        ) : (
          <Select
            value={selectorValue}
            onValueChange={(value) => {
              if (value === "__auto__") {
                onSelectionModeChange("auto");
                return;
              }
              const device = devices.find((candidate) => candidate.deviceId === value);
              if (!device) return;
              onDeviceSelect(value, device.label);
              onSelectionModeChange("specific");
            }}
          >
            <SelectTrigger className="w-full">
              <SelectValue placeholder={t("microphoneSettings.selectPlaceholder")}>
                {isAuto
                  ? t("microphoneSettings.auto.label")
                  : selectedDevice?.label || unavailableLabel}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__auto__">{t("microphoneSettings.auto.label")}</SelectItem>
              {!isAuto && !selectedDevice && (
                <SelectItem value="__unavailable__" disabled>
                  {unavailableLabel}
                </SelectItem>
              )}
              {physicalDevices.map((device) => (
                <SelectItem key={device.deviceId} value={device.deviceId}>
                  {device.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}

        <p className="text-xs text-muted-foreground">{t("microphoneSettings.auto.description")}</p>
        {!isAuto && (
          <p className="text-xs text-muted-foreground">{t("microphoneSettings.helpText")}</p>
        )}
      </div>

      {(isAuto || (microphoneSelectionMode === "built-in" && builtInDevice)) && (
        <div className="p-3 bg-success/10 dark:bg-success/20 border border-success/30 rounded-lg">
          <div className="flex items-center gap-2">
            <Mic className="w-4 h-4 text-success dark:text-success" />
            <span className="text-sm text-success dark:text-success" aria-live="polite">
              {isAuto
                ? t("microphoneSettings.auto.preferredDevice", { device: resolvedLabel })
                : t("microphoneSettings.using", { device: builtInDevice.label })}
            </span>
          </div>
        </div>
      )}

      {microphoneSelectionMode === "built-in" && !builtInDevice && devices.length > 0 && (
        <div className="p-3 bg-warning/10 dark:bg-warning/20 border border-warning/30 rounded-lg">
          <p className="text-sm text-warning dark:text-warning">
            {t("microphoneSettings.noBuiltInDetected")}
          </p>
        </div>
      )}
    </div>
  );
};

export default MicrophoneSettings;
