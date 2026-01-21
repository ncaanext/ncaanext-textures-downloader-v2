interface ExistingFolderDialogProps {
  folderName: string;
  onBackup: () => void;
  onDelete: () => void;
  onCancel: () => void;
}

function ExistingFolderDialog({
  folderName,
  onBackup,
  onDelete,
  onCancel,
}: ExistingFolderDialogProps) {
  return (
    <div className="fixed inset-0 bg-black/70 flex items-center justify-center p-4 z-50">
      <div className="bg-zinc-800 rounded-lg p-6 max-w-md w-full border border-zinc-700 shadow-xl">
        <h3 className="text-lg font-semibold text-zinc-100 mb-2">
          Existing Folder Found
        </h3>
        <p className="text-zinc-300 mb-4">
          An existing <span className="font-mono text-blue-400">{folderName}</span> folder
          was found. What would you like to do?
        </p>

        <div className="space-y-2">
          <button
            onClick={onBackup}
            className="w-full py-2.5 px-4 bg-blue-600 hover:bg-blue-500 text-white rounded-lg
                       font-medium transition-colors text-left flex items-start gap-3"
          >
            <span className="text-blue-200">1.</span>
            <div>
              <div>Back up existing folder</div>
              <div className="text-xs text-blue-200 font-normal mt-0.5">
                Rename to {folderName}_backup_[timestamp]
              </div>
            </div>
          </button>

          <button
            onClick={onDelete}
            className="w-full py-2.5 px-4 bg-zinc-700 hover:bg-zinc-600 text-zinc-100 rounded-lg
                       font-medium transition-colors text-left flex items-start gap-3"
          >
            <span className="text-zinc-400">2.</span>
            <div>
              <div>Delete existing folder</div>
              <div className="text-xs text-zinc-400 font-normal mt-0.5">
                Permanently remove the existing folder
              </div>
            </div>
          </button>

          <button
            onClick={onCancel}
            className="w-full py-2.5 px-4 bg-zinc-900 hover:bg-zinc-800 text-zinc-400 rounded-lg
                       font-medium transition-colors border border-zinc-700"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

export default ExistingFolderDialog;
