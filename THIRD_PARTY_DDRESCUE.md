# Componente de terceros: GNU ddrescue

Sectora empaqueta o invoca **GNU ddrescue** para clonar discos dañados con mapfile (pausa/reanudación).

- Proyecto: https://www.gnu.org/software/ddrescue/
- Licencia: **GPLv2 o posterior**
- Autor: Antonio Diaz Diaz

Sectora (licencia MIT) se distribuye por separado del binario de ddrescue. Si redistribuyes el sidecar `ddrescue-*`, debes cumplir la GPL de ese componente (incluido ofrecer el código fuente correspondiente de ddrescue).

Los binarios se obtienen con `scripts/fetch-ddrescue.sh` o se instalan en el sistema (`brew install ddrescue`, `apt install gddrescue`, etc.).
