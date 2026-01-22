import { APP_TITLE, REPO_NAME, TARGET_FOLDER } from "../config";

function Header() {
  return (
    <header className="text-center">
      <h1 className="text-2xl font-bold text-zinc-100">
        {APP_TITLE}
      </h1>
      <p className="text-sm text-zinc-400 mt-1">
        {REPO_NAME} <span className="text-zinc-600 mx-2">|</span> {TARGET_FOLDER}
      </p>
    </header>
  );
}

export default Header;
