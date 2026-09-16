import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Brain, Keyboard, Mic, Shield, Sliders } from "./icons";
import SettingsPage, { type SettingsSectionType } from "./SettingsPage";
import SidebarModal, { type SidebarItem } from "./ui/SidebarModal";

export type { SettingsSectionType };

// History and audio upload link directly to their transcription settings.
const SECTION_ALIASES: Record<string, SettingsSectionType> = {
  transcription: "speechToText",
  uploadTranscription: "speechToText",
};

interface SettingsModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  initialSection?: string;
}

export default function SettingsModal({ open, onOpenChange, initialSection }: SettingsModalProps) {
  const { t } = useTranslation();
  const sidebarItems: SidebarItem<SettingsSectionType>[] = useMemo(() => {
    const items: SidebarItem<SettingsSectionType>[] = [
      {
        id: "general",
        label: t("settingsModal.sections.general.label"),
        icon: Sliders,
        description: t("settingsModal.sections.general.description"),
        group: t("settingsModal.groups.app"),
      },
      {
        id: "hotkeys",
        label: t("settingsModal.sections.hotkeys.label"),
        icon: Keyboard,
        description: t("settingsModal.sections.hotkeys.description"),
        group: t("settingsModal.groups.app"),
      },
      {
        id: "speechToText",
        label: t("settingsModal.sections.speechToText.label"),
        icon: Mic,
        description: t("settingsModal.sections.speechToText.description"),
        group: t("settingsModal.groups.aiModels"),
      },
      {
        id: "llms",
        label: t("settingsModal.sections.llms.label"),
        icon: Brain,
        description: t("settingsModal.sections.llms.description"),
        group: t("settingsModal.groups.aiModels"),
      },
      {
        id: "privacyData",
        label: t("settingsModal.sections.privacyData.label"),
        icon: Shield,
        description: t("settingsModal.sections.privacyData.description"),
        group: t("settingsModal.groups.system"),
      },
    ];
    return items;
  }, [t]);

  const resolveSection = (section: string | undefined): SettingsSectionType => {
    if (!section) return "general";
    const resolved = SECTION_ALIASES[section] ?? section;
    return sidebarItems.find((item) => item.id === resolved)?.id ?? "general";
  };

  const [activeSection, setActiveSection] = React.useState<SettingsSectionType>(() =>
    resolveSection(initialSection)
  );
  const [prevOpen, setPrevOpen] = useState(open);

  if (open && !prevOpen && initialSection) {
    setPrevOpen(open);
    setActiveSection(resolveSection(initialSection));
  } else if (open !== prevOpen) {
    setPrevOpen(open);
  }

  const handleSectionChange = (section: SettingsSectionType) => {
    setActiveSection(section);
  };

  return (
    <SidebarModal<SettingsSectionType>
      open={open}
      onOpenChange={onOpenChange}
      title={t("settingsModal.title")}
      sidebarItems={sidebarItems}
      activeSection={activeSection}
      onSectionChange={handleSectionChange}
    >
      <SettingsPage activeSection={activeSection} />
    </SidebarModal>
  );
}
