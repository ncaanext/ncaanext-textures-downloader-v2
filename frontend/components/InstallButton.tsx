interface InstallButtonProps {
  onClick: () => void;
  disabled?: boolean;
  isInstalling?: boolean;
}

function InstallButton({ onClick, disabled, isInstalling }: InstallButtonProps) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="w-full mt-6 py-3 px-4 bg-blue-600 hover:bg-blue-500 text-white rounded-lg
                 font-semibold transition-colors
                 disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-blue-600
                 flex items-center justify-center gap-2"
    >
      {isInstalling ? (
        <>
          <svg
            className="animate-spin h-5 w-5"
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 24 24"
          >
            <circle
              className="opacity-25"
              cx="12"
              cy="12"
              r="10"
              stroke="currentColor"
              strokeWidth="4"
            />
            <path
              className="opacity-75"
              fill="currentColor"
              d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
            />
          </svg>
          Installing...
        </>
      ) : (
        "Start Installation"
      )}
    </button>
  );
}

export default InstallButton;
