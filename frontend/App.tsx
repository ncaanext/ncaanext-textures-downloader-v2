import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Header from "./components/Header";
import DirectoryPicker from "./components/DirectoryPicker";
import InstallButton from "./components/InstallButton";
import ProgressDisplay from "./components/ProgressDisplay";
import ExistingFolderDialog from "./components/ExistingFolderDialog";
import { TARGET_FOLDER } from "./config";

interface ProgressPayload {
  stage: string;
  message: string;
  percent: number | null;
}

type InstallStatus = "idle" | "installing" | "complete" | "error";

function App() {
  const [texturesDir, setTexturesDir] = useState("");
  const [showFolderDialog, setShowFolderDialog] = useState(false);
  const [installStatus, setInstallStatus] = useState<InstallStatus>("idle");
  const [progressMessages, setProgressMessages] = useState<string[]>([]);
  const [progressPercent, setProgressPercent] = useState<number | null>(null);
  const [currentStage, setCurrentStage] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [gitAvailable, setGitAvailable] = useState<boolean | null>(null);
  const [gitError, setGitError] = useState<string>("");

  // Check if git is available on mount
  useEffect(() => {
    const checkGit = async () => {
      try {
        const available = await invoke<boolean>("check_git_installed");
        setGitAvailable(available);
        if (!available) {
          const error = await invoke<string>("get_git_error");
          setGitError(error);
        }
      } catch (e) {
        setGitAvailable(false);
        setGitError("Failed to check git availability");
      }
    };
    checkGit();
  }, []);

  // Listen for progress events
  useEffect(() => {
    const unlisten = listen<ProgressPayload>("install-progress", (event) => {
      const { stage, message, percent } = event.payload;

      setCurrentStage(stage);
      setProgressMessages((prev) => [...prev, message]);
      if (percent !== null) {
        setProgressPercent(percent);
      }

      if (stage === "complete") {
        setInstallStatus("complete");
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleStartInstall = async () => {
    if (!texturesDir) {
      setErrorMessage("Please select a textures directory first");
      return;
    }

    // Check if folder exists
    try {
      const exists = await invoke<boolean>("check_existing_folder", {
        texturesDir,
      });

      if (exists) {
        setShowFolderDialog(true);
        return;
      }

      // No existing folder, start installation
      await startInstallation();
    } catch (e) {
      setErrorMessage(`Error: ${e}`);
    }
  };

  const startInstallation = async () => {
    setInstallStatus("installing");
    setProgressMessages([]);
    setProgressPercent(0);
    setCurrentStage(null);
    setErrorMessage(null);

    try {
      await invoke("start_installation", { texturesDir });
    } catch (e) {
      setInstallStatus("error");
      setErrorMessage(`Installation failed: ${e}`);
    }
  };

  const handleBackup = async () => {
    setShowFolderDialog(false);
    try {
      const backupName = await invoke<string>("backup_existing_folder", {
        texturesDir,
      });
      setProgressMessages([`Backed up existing folder to: ${backupName}`]);
      await startInstallation();
    } catch (e) {
      setErrorMessage(`Backup failed: ${e}`);
    }
  };

  const handleDelete = async () => {
    setShowFolderDialog(false);
    try {
      await invoke("delete_existing_folder", { texturesDir });
      setProgressMessages(["Deleted existing folder"]);
      await startInstallation();
    } catch (e) {
      setErrorMessage(`Delete failed: ${e}`);
    }
  };

  const handleCancel = () => {
    setShowFolderDialog(false);
  };

  const isInstalling = installStatus === "installing";

  return (
    <div className="min-h-screen bg-zinc-900 text-zinc-100 p-6 overflow-auto">
      <div className="max-w-xl mx-auto space-y-6">
        <Header />

        {/* Git availability warning */}
        {gitAvailable === false && (
          <div className="bg-red-900/50 border border-red-700 rounded-lg p-4">
            <p className="text-red-200 font-medium">Git not available</p>
            <p className="text-red-300 text-sm mt-1">{gitError}</p>
          </div>
        )}

        {/* First Time Installation Section */}
        <section className="bg-zinc-800 rounded-lg p-5 border border-zinc-700">
          <h2 className="text-lg font-semibold text-zinc-100 mb-4 uppercase tracking-wide">
            First Time Installation
          </h2>

          <DirectoryPicker
            value={texturesDir}
            onChange={setTexturesDir}
            disabled={isInstalling}
          />

          <InstallButton
            onClick={handleStartInstall}
            disabled={!texturesDir || isInstalling || gitAvailable === false}
            isInstalling={isInstalling}
          />

          {/* Error message */}
          {errorMessage && (
            <div className="mt-4 p-3 bg-red-900/30 border border-red-800 rounded text-red-300 text-sm">
              {errorMessage}
            </div>
          )}

          {/* Progress display */}
          {(isInstalling || installStatus === "complete") && (
            <ProgressDisplay
              messages={progressMessages}
              percent={progressPercent}
              stage={currentStage}
              isComplete={installStatus === "complete"}
            />
          )}
        </section>
      </div>

      {/* Existing folder dialog */}
      {showFolderDialog && (
        <ExistingFolderDialog
          folderName={TARGET_FOLDER}
          onBackup={handleBackup}
          onDelete={handleDelete}
          onCancel={handleCancel}
        />
      )}
    </div>
  );
}

export default App;
