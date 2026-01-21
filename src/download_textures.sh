#!/bin/bash

echo "==============================="
echo "NCAA Next Textures Downloader"
echo "==============================="

# Check for git
if ! command -v git &> /dev/null
then
    echo ""
    echo "Git is not installed."
    echo "Run this in Terminal:"
    echo "  xcode-select --install"
    echo "Then re-run this script."
    read -p "Press Enter to exit..."
    exit 1
fi

TEMP_REPO="_temp_ncaa_repo"

# Clean up old temp folder
rm -rf "$TEMP_REPO"

echo ""
echo "Creating temporary repository..."
git clone --depth=1 --filter=blob:none --sparse https://github.com/ncaanext/ncaa-next-26.git "$TEMP_REPO"
cd "$TEMP_REPO" || exit 1

echo ""
echo "Setting sparse checkout to SLUS-21214 folder..."
git sparse-checkout set textures/SLUS-21214

echo ""
echo "Moving folder into final location..."
cd ..
mv "$TEMP_REPO/textures/SLUS-21214" "SLUS-21214"

echo ""
echo "Cleaning up..."
rm -rf "$TEMP_REPO"

echo ""
echo "==============================="
echo "Done!"
echo "Folder installed at:"
pwd
echo "/SLUS-21214"
echo "==============================="
read -p "Press Enter to exit..."
