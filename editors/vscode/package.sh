#!/bin/sh
# Package the extension as symbolasm.vsix (a zip with a manifest; no vsce
# needed), and install it if `code` is around. Run `npm install` first, for
# vscode-languageclient.
#
# Usage: editors/vscode/package.sh

set -e
cd "$(dirname "$0")"
v=$(sed -n 's/.*"version": "\(.*\)".*/\1/p' package.json)
d=$(mktemp -d)
mkdir "$d/extension"
cp -R package.json README.md extension.js language-configuration.json syntaxes node_modules "$d/extension/"
cat > "$d/[Content_Types].xml" <<'X'
<?xml version="1.0" encoding="utf-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension=".json" ContentType="application/json"/><Default Extension=".js" ContentType="application/javascript"/><Default Extension=".vsixmanifest" ContentType="text/xml"/></Types>
X
cat > "$d/extension.vsixmanifest" <<X
<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011" xmlns:d="http://schemas.microsoft.com/developer/vsx-schema-design/2011">
  <Metadata>
    <Identity Language="en-US" Id="symbolasm" Version="$v" Publisher="raphlinus" />
    <DisplayName>symbolasm</DisplayName>
    <Description xml:space="preserve">Language server client for symbolasm</Description>
    <Properties><Property Id="Microsoft.VisualStudio.Code.Engine" Value="^1.91.0" /></Properties>
  </Metadata>
  <Installation><InstallationTarget Id="Microsoft.VisualStudio.Code" /></Installation>
  <Dependencies />
  <Assets><Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" Addressable="true" /></Assets>
</PackageManifest>
X
rm -f symbolasm.vsix
(cd "$d" && zip -q -r "$OLDPWD/symbolasm.vsix" "[Content_Types].xml" extension.vsixmanifest extension)
rm -rf "$d"
CODE=$(command -v code || echo "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code")
if [ -x "$CODE" ]; then
    "$CODE" --install-extension symbolasm.vsix --force
fi
