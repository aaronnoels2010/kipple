# Kipple installeren

*English: [INSTALL.md](INSTALL.md)*

Kipple is gratis en heeft geen betaald certificaat om de code te ondertekenen. Daarom waarschuwen Windows en macOS je de eerste keer dat je het opent. Die waarschuwing betekent "deze app komt niet van een betaald ontwikkelaarsaccount", niet "deze app is schadelijk". Op deze pagina lees je hoe je er één keer langs komt, en hoe je controleert dat je bestand echt het bestand is dat wij gebouwd hebben.

Download Kipple op de [Releases-pagina](https://github.com/aaronnoels2010/kipple/releases/latest). Kies het bestand voor jouw systeem:

| Systeem | Bestand | Opmerking |
|---|---|---|
| **Windows 10/11** | `Kipple_<versie>_x64-setup.exe` | Aanbevolen. Installeert alleen voor jouw account, zonder beheerdersrechten, en werkt zichzelf bij |
| | `Kipple_<versie>_x64_en-US.msi` | Voor beheerders die het voor alle gebruikers installeren |
| | `Kipple_<versie>_x64-portable.zip` | Zonder installatie: uitpakken en `Kipple.exe` starten. Bewaart alles in een map `KippleData` ernaast, dus het werkt vanaf een USB-stick. Werkt zichzelf niet bij |
| **macOS 11 of nieuwer** (Apple silicon en Intel) | `Kipple_<versie>_universal.dmg` | |
| **Linux** | `Kipple_<versie>_amd64.AppImage` | Werkt op de meeste distributies en werkt zichzelf bij |
| | `Kipple_<versie>_amd64.deb` | Debian, Ubuntu, Linux Mint… |
| | `Kipple-<versie>-1.x86_64.rpm` | Fedora, openSUSE… |

## Windows

1. Start de `-setup.exe`.
2. Windows SmartScreen zegt **"Windows heeft uw pc beschermd"**. Klik op **Meer informatie**, controleer dat er *Kipple* staat en klik op **Toch uitvoeren**.
3. Volg het installatieprogramma. Kipple staat daarna in het Startmenu.

Je ziet die waarschuwing alleen bij een nieuwe versie die je nog niet gestart hebt. De draagbare `.exe` toont ze de eerste keer ook.

**Draagbare versie:** pak eerst de hele zip uit (*Alles uitpakken…*) en start dan `Kipple.exe` uit de uitgepakte map `Kipple`. Start je hem vanuit de zip, dan ziet hij zijn bestand `portable` niet en gebruikt hij je gebruikersprofiel, net als de geïnstalleerde versie. Draait de geïnstalleerde Kipple al, dan brengt het starten van de draagbare versie gewoon de geïnstalleerde naar voren: sluit die dan eerst af.

## macOS

1. Open de `.dmg` en sleep **Kipple** naar **Apps**.
2. Open Kipple vanuit Apps. macOS meldt dat het **niet kan controleren** of Kipple vrij is van malware. Klik op **Gereed** (niet op *Verplaats naar prullenmand*).
3. Open **Systeeminstellingen → Privacy en beveiliging**, scrol naar *"Kipple" is geblokkeerd…* en klik op **Open toch**. Bevestig met je wachtwoord of Touch ID.
4. Kipple opent. Vanaf nu opent het gewoon.

Na een update kan macOS het nog één keer vragen (stap 3). De toegang tot mappen die je Kipple gaf, blijft behouden: elke versie is ondertekend met hetzelfde certificaat, *Kipple Release Signing*. Zo controleer je dat jouw exemplaar het heeft:

```sh
codesign -dvv /Applications/Kipple.app 2>&1 | grep Authority
# Authority=Kipple Release Signing
```

## Linux

**AppImage**

```sh
chmod +x Kipple_*_amd64.AppImage
./Kipple_*_amd64.AppImage
```

Start het niet en gaat de melding over FUSE, installeer het dan: `sudo apt install libfuse2t64` (Ubuntu 24.04 en nieuwer) of `sudo apt install libfuse2` (oudere Ubuntu en Debian).

**.deb**: `sudo apt install ./Kipple_*_amd64.deb`

**.rpm**: `sudo dnf install ./Kipple-*.x86_64.rpm`

## Je download controleren

Bij elke versie hoort een `SHA256SUMS.txt` met de controlesom van elk bestand. Download het naar dezelfde map als je bestand en voer dan uit:

**macOS / Linux**

```sh
shasum -a 256 --check --ignore-missing SHA256SUMS.txt
# Kipple_…: OK
```

**Windows (PowerShell)**

```powershell
(Get-FileHash .\Kipple_<versie>_x64-setup.exe -Algorithm SHA256).Hash.ToLower()
```

Vergelijk het resultaat met de regel voor dat bestand in `SHA256SUMS.txt`: ze moeten exact gelijk zijn.

**Bewijs dat het uit deze repository komt.** Elk bestand heeft ook een GitHub-bouwattest, dat bewijst dat GitHub Actions het gebouwd heeft uit de broncode van Kipple. Met de [GitHub CLI](https://cli.github.com):

```sh
gh attestation verify Kipple_<versie>_universal.dmg --repo aaronnoels2010/kipple
```

## Kipple verwijderen

- **Windows:** *Instellingen → Apps → Geïnstalleerde apps → Kipple → Verwijderen*.
- **macOS:** sleep Kipple vanuit Apps naar de prullenmand.
- **Linux:** verwijder de AppImage, of `sudo apt remove kipple` / `sudo dnf remove kipple`.

De instellingen en index van Kipple blijven daarna in je gebruikersprofiel staan (de draagbare versie bewaart ze in haar map `KippleData`: verwijder die map ook). **Instellingen → Alle Kipple-gegevens verwijderen** haalt ze vanuit de app weg (komt in een latere versie, voor 1.0).
