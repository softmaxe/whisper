import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getDefaultPromptText,
  resolvePromptTemplate,
  wrapCleanupTranscript,
} from "../../config/prompts";
import { useDialogs } from "../../hooks/useDialogs";
import ReasoningService from "../../services/ReasoningService";
import { useSettingsStore } from "../../stores/settingsStore";
import { useAgentName } from "../../utils/agentName";
import { resolveCleanupLanguage } from "../../utils/chineseScript";
import logger from "../../utils/logger";
import { getDictionaryHintWords } from "../../utils/snippets";
import { AlertTriangle, Check, Copy, Edit3, Eye, Play, RotateCcw, Save, TestTube } from "../icons";
import { Button } from "./button";
import { AlertDialog } from "./dialog";
import { Textarea } from "./textarea";

interface PromptStudioProps {
  className?: string;
}

export default function PromptStudio({ className = "" }: PromptStudioProps) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<"current" | "edit" | "test">("current");
  const [testText, setTestText] = useState(() => t("promptStudio.defaultTestInput"));
  const [testResult, setTestResult] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [copiedPrompt, setCopiedPrompt] = useState(false);

  const { alertDialog, showAlertDialog, hideAlertDialog } = useDialogs();
  const { agentName } = useAgentName();
  const effectiveSettings = useSettingsStore((state) => state);
  const { uiLanguage, useCleanupModel, cleanupModel, cleanupRemoteUrl } = effectiveSettings;
  const customPrompt = useSettingsStore((s) => s.customPrompts.cleanup);
  const setCustomPrompt = useSettingsStore((s) => s.setCustomPrompt);
  const defaultPrompt = getDefaultPromptText("cleanup", uiLanguage);
  const [editedPrompt, setEditedPrompt] = useState(customPrompt || defaultPrompt);

  const savePrompt = () => {
    // Keep the shipped default eligible for future prompt updates.
    setCustomPrompt("cleanup", editedPrompt === defaultPrompt ? "" : editedPrompt);
    showAlertDialog({
      title: t("promptStudio.dialogs.saved.title"),
      description: t("promptStudio.dialogs.saved.description"),
    });
  };

  const resetToDefault = () => {
    setEditedPrompt(defaultPrompt);
    setCustomPrompt("cleanup", "");
    showAlertDialog({
      title: t("promptStudio.dialogs.reset.title"),
      description: t("promptStudio.dialogs.reset.description"),
    });
  };

  const copyText = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedPrompt(true);
    setTimeout(() => setCopiedPrompt(false), 2000);
  };

  const testPrompt = async () => {
    if (!testText.trim()) return;

    setIsLoading(true);
    setTestResult("");

    try {
      if (!useCleanupModel) {
        setTestResult(t("promptStudio.test.disabledReasoning"));
        return;
      }
      if (!cleanupRemoteUrl.trim()) {
        setTestResult(t("promptStudio.test.noEndpoint"));
        return;
      }
      if (!cleanupModel.trim()) {
        setTestResult(t("promptStudio.test.noModelSelected"));
        return;
      }

      const result = await ReasoningService.processText(
        wrapCleanupTranscript(testText),
        cleanupModel.trim(),
        agentName,
        {
          lanUrl: cleanupRemoteUrl.trim(),
          customApiKey: effectiveSettings.cleanupCustomApiKey,
          inferenceScope: "dictationCleanup",
          disableThinking: effectiveSettings.cleanupDisableThinking,
          temperature: 0,
          requireCompleteOutput: true,
          systemPrompt: resolvePromptTemplate(editedPrompt || defaultPrompt, {
            agentName,
            language: resolveCleanupLanguage(effectiveSettings.preferredLanguage),
            customDictionary: getDictionaryHintWords(effectiveSettings),
            uiLanguage,
          }),
        }
      );
      setTestResult(result);
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      logger.error("PromptStudio test failed", { error: errorMessage }, "prompt-studio");
      setTestResult(t("promptStudio.test.failed", { error: errorMessage }));
    } finally {
      setIsLoading(false);
    }
  };

  const isCustomPrompt = customPrompt.length > 0;
  const currentPrompt = customPrompt || defaultPrompt;

  const tabs = [
    { id: "current" as const, label: t("promptStudio.tabs.view"), icon: Eye },
    { id: "edit" as const, label: t("promptStudio.tabs.customize"), icon: Edit3 },
    { id: "test" as const, label: t("promptStudio.tabs.test"), icon: TestTube },
  ];

  return (
    <div className={className}>
      <AlertDialog
        open={alertDialog.open}
        onOpenChange={(open) => !open && hideAlertDialog()}
        title={alertDialog.title}
        description={alertDialog.description}
        onOk={() => {}}
      />

      {/* Tab Navigation + Content in a single panel */}
      <div className="rounded-xl border border-border/70 dark:border-border-subtle bg-card dark:bg-surface-2 overflow-hidden">
        <div className="flex border-b border-border/70 dark:border-border-subtle">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex-1 flex items-center justify-center gap-2 px-4 py-3 text-xs font-medium transition-colors duration-150 border-b-2 ${
                  isActive
                    ? "border-primary text-foreground bg-primary/5 dark:bg-primary/3"
                    : "border-transparent text-muted-foreground hover:text-foreground hover:bg-black/2 dark:hover:bg-white/2"
                }`}
              >
                <Icon className="w-3.5 h-3.5" />
                {tab.label}
              </button>
            );
          })}
        </div>

        {/* ── View Tab ── */}
        {activeTab === "current" && (
          <div className="divide-y divide-border/60 dark:divide-border-subtle">
            <div className="px-5 py-4">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <p className="text-xs font-medium text-muted-foreground/70 uppercase tracking-wider">
                    {isCustomPrompt
                      ? t("promptStudio.view.customPrompt")
                      : t("promptStudio.view.defaultPrompt")}
                  </p>
                  {isCustomPrompt && (
                    <span className="text-xs font-semibold uppercase tracking-wider px-1.5 py-px rounded-full bg-primary/10 text-primary">
                      {t("promptStudio.view.modified")}
                    </span>
                  )}
                </div>
                <Button
                  onClick={() => copyText(currentPrompt)}
                  variant="ghost"
                  size="sm"
                  className="h-7 px-2 text-xs"
                >
                  {copiedPrompt ? (
                    <>
                      <Check className="w-3 h-3 me-1 text-success" />{" "}
                      {t("promptStudio.common.copied")}
                    </>
                  ) : (
                    <>
                      <Copy className="w-3 h-3 me-1" /> {t("promptStudio.common.copy")}
                    </>
                  )}
                </Button>
              </div>
              <div className="bg-muted/30 dark:bg-surface-raised/30 border border-border/70 rounded-lg p-4 max-h-80 overflow-y-auto">
                <pre
                  dir="auto"
                  className="text-xs font-mono text-muted-foreground whitespace-pre-wrap leading-relaxed"
                >
                  {currentPrompt.replace(/\{\{agentName\}\}/g, agentName)}
                </pre>
              </div>
            </div>
          </div>
        )}

        {/* ── Edit Tab ── */}
        {activeTab === "edit" && (
          <div className="divide-y divide-border/60 dark:divide-border-subtle">
            <div className="px-5 py-4">
              <p className="text-xs text-muted-foreground leading-relaxed">
                <span className="font-medium text-warning">
                  {t("promptStudio.edit.cautionLabel")}
                </span>{" "}
                {t("promptStudio.edit.cautionTextPrefix")}
              </p>
            </div>

            <div className="px-5 py-4">
              <Textarea
                dir="auto"
                value={editedPrompt}
                onChange={(e) => setEditedPrompt(e.target.value)}
                rows={16}
                className="font-mono text-xs leading-relaxed"
                placeholder={t("promptStudio.edit.placeholder")}
              />
            </div>

            <div className="px-5 py-4">
              <div className="flex gap-2">
                <Button onClick={savePrompt} size="sm" className="flex-1">
                  <Save className="w-3.5 h-3.5 me-2" />
                  {t("promptStudio.common.save")}
                </Button>
                <Button onClick={resetToDefault} variant="outline" size="sm">
                  <RotateCcw className="w-3.5 h-3.5 me-2" />
                  {t("promptStudio.common.reset")}
                </Button>
              </div>
            </div>
          </div>
        )}

        {activeTab === "test" && (
          <div className="divide-y divide-border/60 dark:divide-border-subtle">
            {!useCleanupModel && (
              <div className="px-5 py-4">
                <div className="rounded-lg border border-warning/20 bg-warning/5 dark:bg-warning/10 px-4 py-3">
                  <div className="flex items-start gap-2.5">
                    <AlertTriangle className="w-3.5 h-3.5 text-warning mt-0.5 shrink-0" />
                    <p className="text-xs text-muted-foreground leading-relaxed">
                      {t("promptStudio.test.disabledReasoning")}
                    </p>
                  </div>
                </div>
              </div>
            )}

            <div className="px-5 py-4 space-y-2">
              <div className="flex items-center gap-2">
                <p className="text-xs text-muted-foreground/70 uppercase tracking-wider">
                  {t("settingsPage.selfHosted.serverUrl")}
                </p>
                <p dir="ltr" className="text-xs font-medium text-foreground font-mono break-all">
                  {cleanupRemoteUrl.trim() || t("promptStudio.test.none")}
                </p>
              </div>
              <div className="flex items-center gap-2">
                <p className="text-xs text-muted-foreground/70 uppercase tracking-wider">
                  {t("promptStudio.test.modelLabel")}
                </p>
                <p dir="ltr" className="text-xs font-medium text-foreground font-mono break-all">
                  {cleanupModel.trim() || t("promptStudio.test.none")}
                </p>
              </div>
            </div>

            <div className="px-5 py-4">
              <p className="text-xs font-medium text-foreground mb-2">
                {t("promptStudio.test.inputLabel")}
              </p>
              <Textarea
                dir="auto"
                value={testText}
                onChange={(e) => setTestText(e.target.value)}
                rows={3}
                className="text-xs"
                placeholder={t("promptStudio.test.inputPlaceholder")}
              />
            </div>

            <div className="px-5 py-4">
              <Button
                onClick={testPrompt}
                disabled={!testText.trim() || isLoading || !useCleanupModel}
                size="sm"
                className="w-full"
              >
                <Play className="w-3.5 h-3.5 me-2" />
                {isLoading ? t("promptStudio.test.processing") : t("promptStudio.test.run")}
              </Button>
            </div>

            {testResult && (
              <div className="px-5 py-4">
                <div className="flex items-center justify-between mb-2">
                  <p className="text-xs font-medium text-foreground">
                    {t("promptStudio.test.outputLabel")}
                  </p>
                  <Button
                    onClick={() => copyText(testResult)}
                    variant="ghost"
                    size="icon"
                    className="h-6 w-7"
                  >
                    <Copy className="w-3 h-3 text-muted-foreground" />
                  </Button>
                </div>
                <div className="bg-muted/30 dark:bg-surface-raised/30 border border-border/70 rounded-lg p-4 max-h-48 overflow-y-auto">
                  <pre
                    dir="auto"
                    className="text-xs text-foreground whitespace-pre-wrap leading-relaxed"
                  >
                    {testResult}
                  </pre>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
