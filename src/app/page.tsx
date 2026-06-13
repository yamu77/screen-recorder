"use client";

import { invoke } from "@tauri-apps/api/core";
import {
  ArrowLeft,
  Camera,
  Moon,
  RefreshCw,
  Settings,
  Square,
  Sun,
  Video,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import styles from "./page.module.css";

type WindowInfo = {
  id: number;
  title: string;
  app_name: string;
  pid: number;
};

type ThemeMode = "light" | "dark";
type ViewMode = "recorder" | "settings";
type StatusTone = "idle" | "success" | "error";

const themeStorageKey = "screen-recorder-theme";

export default function Home() {
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [selectedWindowId, setSelectedWindowId] = useState("");
  const [themeMode, setThemeMode] = useState<ThemeMode>("light");
  const [viewMode, setViewMode] = useState<ViewMode>("recorder");
  const [isLoadingWindows, setIsLoadingWindows] = useState(false);
  const [isRecording, setIsRecording] = useState(false);
  const [status, setStatus] = useState({
    tone: "idle" as StatusTone,
    message: "録画またはキャプチャするウィンドウを選択してください。",
  });

  const selectedWindow = useMemo(
    () => windows.find((window) => String(window.id) === selectedWindowId),
    [selectedWindowId, windows],
  );

  const visibleWindows = useMemo(
    () =>
      windows.map((window) => ({
        ...window,
        displayTitle: window.title || "(タイトルなし)",
      })),
    [windows],
  );

  const loadWindows = useCallback(async () => {
    setIsLoadingWindows(true);
    try {
      const data = await invoke<WindowInfo[]>("get_windows");
      setWindows(data);

      if (data.length === 0) {
        setSelectedWindowId("");
        setStatus({
          tone: "idle",
          message: "選択できるウィンドウが見つかりませんでした。",
        });
        return;
      }

      setSelectedWindowId((currentId) => {
        const stillExists = data.some(
          (window) => String(window.id) === currentId,
        );
        return stillExists ? currentId : String(data[0].id);
      });
      setStatus({
        tone: "idle",
        message: "ウィンドウ一覧を更新しました。",
      });
    } catch (error) {
      console.error("Failed to load windows:", error);
      setStatus({
        tone: "error",
        message: `ウィンドウ一覧の取得に失敗しました: ${String(error)}`,
      });
    } finally {
      setIsLoadingWindows(false);
    }
  }, []);

  useEffect(() => {
    const savedTheme = window.localStorage.getItem(themeStorageKey);
    if (savedTheme === "light" || savedTheme === "dark") {
      setThemeMode(savedTheme);
    }

    void loadWindows();
  }, [loadWindows]);

  useEffect(() => {
    document.documentElement.dataset.theme = themeMode;
    window.localStorage.setItem(themeStorageKey, themeMode);
  }, [themeMode]);

  const requireSelectedWindow = () => {
    if (selectedWindow) {
      return selectedWindow;
    }

    setStatus({
      tone: "error",
      message: "先にキャプチャまたは録画するウィンドウを選択してください。",
    });
    return null;
  };

  const handleCapture = async () => {
    const targetWindow = requireSelectedWindow();
    if (!targetWindow) return;

    try {
      setStatus({
        tone: "idle",
        message: "スクリーンショットを保存しています...",
      });
      const result = await invoke<string>("capture_selected_window", {
        title: targetWindow.title,
      });
      setStatus({ tone: "success", message: result });
    } catch (error) {
      console.error("Failed to capture window:", error);
      setStatus({
        tone: "error",
        message: `キャプチャに失敗しました: ${String(error)}`,
      });
    }
  };

  const handleStartRecord = async () => {
    const targetWindow = requireSelectedWindow();
    if (!targetWindow) return;

    try {
      setStatus({ tone: "idle", message: "録画を開始しています..." });
      const result = await invoke<string>("start_record_window", {
        title: targetWindow.title,
        pid: targetWindow.pid,
      });
      setIsRecording(true);
      setStatus({ tone: "success", message: result });
    } catch (error) {
      console.error("Failed to start recording:", error);
      setStatus({
        tone: "error",
        message: `録画開始に失敗しました: ${String(error)}`,
      });
    }
  };

  const handleStopRecord = async () => {
    try {
      setStatus({ tone: "idle", message: "録画を停止しています..." });
      const result = await invoke<string>("stop_record_window");
      setIsRecording(false);
      setStatus({ tone: "success", message: result });
    } catch (error) {
      console.error("Failed to stop recording:", error);
      setStatus({
        tone: "error",
        message: `録画停止に失敗しました: ${String(error)}`,
      });
    }
  };

  return (
    <div className={styles.page} data-theme={themeMode}>
      <aside className={styles.sidebar} aria-label="アプリナビゲーション">
        <div className={styles.logoMark}>SR</div>
      </aside>

      <button
        type="button"
        className={`${styles.settingsFab} ${viewMode === "settings" ? styles.settingsFabActive : ""}`}
        onClick={() => setViewMode("settings")}
        aria-label="設定を開く"
        title="設定"
      >
        <Settings className={styles.settingsIcon} size={23} strokeWidth={2.4} />
      </button>

      <main className={styles.main}>
        {viewMode === "recorder" ? (
          <RecorderView
            handleCapture={handleCapture}
            handleStartRecord={handleStartRecord}
            handleStopRecord={handleStopRecord}
            isLoadingWindows={isLoadingWindows}
            isRecording={isRecording}
            loadWindows={loadWindows}
            selectedWindow={selectedWindow}
            selectedWindowId={selectedWindowId}
            setSelectedWindowId={setSelectedWindowId}
            status={status}
            visibleWindows={visibleWindows}
          />
        ) : (
          <SettingsView
            setThemeMode={setThemeMode}
            setViewMode={setViewMode}
            themeMode={themeMode}
          />
        )}
      </main>
    </div>
  );
}

function RecorderView({
  handleCapture,
  handleStartRecord,
  handleStopRecord,
  isLoadingWindows,
  isRecording,
  loadWindows,
  selectedWindow,
  selectedWindowId,
  setSelectedWindowId,
  status,
  visibleWindows,
}: {
  handleCapture: () => void;
  handleStartRecord: () => void;
  handleStopRecord: () => void;
  isLoadingWindows: boolean;
  isRecording: boolean;
  loadWindows: () => void;
  selectedWindow?: WindowInfo;
  selectedWindowId: string;
  setSelectedWindowId: (value: string) => void;
  status: { tone: StatusTone; message: string };
  visibleWindows: Array<WindowInfo & { displayTitle: string }>;
}) {
  return (
    <section className={styles.workspace} aria-label="録画操作">
      <div className={styles.topBar}>
        <div
          className={`${styles.recordingPill} ${isRecording ? styles.recordingPillActive : ""}`}
        >
          <span />
          {isRecording ? "録画中" : "待機中"}
        </div>
      </div>

      <div className={styles.panelGrid}>
        <section className={`${styles.panel} ${styles.windowPanel}`}>
          <div className={styles.windowSelector}>
            <div className={styles.windowSelectorHeader}>
              <label className={styles.fieldLabel} htmlFor="window-select">
                ウィンドウ
              </label>
              <button
                type="button"
                className={styles.iconTextButton}
                onClick={loadWindows}
                disabled={isLoadingWindows}
              >
                <RefreshCw
                  size={18}
                  className={isLoadingWindows ? styles.spin : undefined}
                />
                更新
              </button>
            </div>

            <select
              id="window-select"
              className={styles.windowSelect}
              value={selectedWindowId}
              onChange={(event) => setSelectedWindowId(event.target.value)}
            >
              {visibleWindows.length === 0 ? (
                <option value="">ウィンドウがありません</option>
              ) : (
                visibleWindows.map((window) => (
                  <option key={window.id} value={String(window.id)}>
                    {window.displayTitle}
                  </option>
                ))
              )}
            </select>
          </div>

          <div className={styles.windowPreview}>
            <div className={styles.previewIcon}>
              <Video size={28} />
            </div>
            <div>
              <p className={styles.previewTitle}>
                {selectedWindow?.title || "ウィンドウ未選択"}
              </p>
              <p className={styles.previewMeta}>
                {selectedWindow
                  ? `${selectedWindow.app_name || "Unknown app"} / PID ${selectedWindow.pid}`
                  : "一覧から対象を選んでください。"}
              </p>
            </div>
          </div>
        </section>

        <section className={`${styles.panel} ${styles.actionsPanel}`}>
          <div className={styles.actionGrid}>
            <button
              type="button"
              className={styles.primaryAction}
              onClick={handleCapture}
            >
              <Camera size={22} />
              キャプチャ
            </button>
            <button
              type="button"
              className={
                isRecording ? styles.dangerAction : styles.primaryAction
              }
              onClick={isRecording ? handleStopRecord : handleStartRecord}
            >
              {isRecording ? <Square size={20} /> : <Video size={22} />}
              {isRecording ? "録画停止" : "録画開始"}
            </button>
          </div>
        </section>
      </div>

      <div className={`${styles.statusBar} ${styles[`status_${status.tone}`]}`}>
        <span />
        {status.message}
      </div>
    </section>
  );
}

function SettingsView({
  setThemeMode,
  setViewMode,
  themeMode,
}: {
  setThemeMode: (mode: ThemeMode) => void;
  setViewMode: (mode: ViewMode) => void;
  themeMode: ThemeMode;
}) {
  return (
    <section className={styles.settingsPage} aria-label="設定">
      <button
        type="button"
        className={styles.backButton}
        onClick={() => setViewMode("recorder")}
      >
        <ArrowLeft size={18} />
        操作画面へ戻る
      </button>

      <div className={styles.settingsHeader}>
        <p className={styles.kicker}>Settings</p>
        <h1>設定</h1>
        <p className={styles.lead}>
          画面テーマをライトモードまたはダークモードに切り替えます。
        </p>
      </div>

      <section className={styles.panel}>
        <div className={styles.panelHeader}>
          <div>
            <h2>表示テーマ</h2>
            <p>選択内容はこの端末に保存されます。</p>
          </div>
        </div>

        <div className={styles.themeChoices}>
          <button
            type="button"
            className={`${styles.themeChoice} ${themeMode === "light" ? styles.themeChoiceActive : ""}`}
            onClick={() => setThemeMode("light")}
            aria-pressed={themeMode === "light"}
          >
            <Sun size={24} />
            <span>
              <strong>ライトモード</strong>
              <small>明るく、コントラストを抑えた操作画面</small>
            </span>
          </button>

          <button
            type="button"
            className={`${styles.themeChoice} ${themeMode === "dark" ? styles.themeChoiceActive : ""}`}
            onClick={() => setThemeMode("dark")}
            aria-pressed={themeMode === "dark"}
          >
            <Moon size={24} />
            <span>
              <strong>ダークモード</strong>
              <small>暗い環境でも見やすい操作画面</small>
            </span>
          </button>
        </div>
      </section>
    </section>
  );
}
