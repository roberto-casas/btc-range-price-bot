# Validacion de la Estrategia Delta-Neutral BTC Range para Polymarket

**Fecha:** 2026-03-08 (v2 - con costes reales)
**Autor:** Analisis automatizado
**Datos:** 821 velas diarias BTC/USD (Ene 2023 - Abr 2025)

---

## 1. Resumen de la Estrategia

La estrategia explota **dos mercados de prediccion emparejados** en Polymarket para beneficiarse cuando Bitcoin permanece dentro de un rango de precio:

- **Pierna LOW:** COMPRAR YES en "BTC por encima de $X" (donde X esta por debajo del precio actual)
- **Pierna HIGH:** COMPRAR NO en "BTC por encima de $Y" (donde Y esta por encima del precio actual)
- **Condicion de ganancia:** BTC permanece dentro del rango [$X, $Y] al vencimiento

### Ejemplo REALISTA con BTC a $68,000 y rango 88-112%:

| Concepto | Valor |
|----------|-------|
| LOW: "BTC > $59,840" (88% spot) | YES price = $0.85 |
| HIGH: "BTC > $76,160" (112% spot) | YES price = $0.15, NO price = $0.85 |
| Coste nominal por unidad | $0.85 + $0.85 = $1.70 |
| Spread (2%) + Slippage (0.5%) | +$0.0425 por pierna |
| **Coste efectivo por unidad** | **$1.7425** |
| Beneficio neto si en rango | $2.00 - $1.7425 = **$0.2575** |
| **ROI efectivo por trade** | **14.8%** |
| Perdida maxima | $1.7425 (el coste invertido) |
| **Breakeven Win Rate necesario** | **87.1%** |

---

## 2. Mercados Disponibles en Polymarket

### Tipos de mercados Bitcoin activos:

| Tipo | Timeframe | Mercados activos | Relevancia |
|------|-----------|-----------------|------------|
| **"Bitcoin above __ on [fecha]?"** | Diario | 7 | **IDEAL** - Es lo que usa el bot |
| **"Bitcoin above __ this week?"** | Semanal | **63** | **OPTIMO** - Mejor liquidez |
| **"Bitcoin price on [fecha]?"** | Diario | 7 | Alternativa (multiples rangos) |
| **"What price will Bitcoin hit in [mes]?"** | Mensual | 22 | No compatible directamente |
| **"Bitcoin Up or Down"** | 5-15 min | Muchos | Fees altas (hasta 3%), no aplica |

### Conclusion: Priorizar mercados semanales (63 activos, mejor liquidez, 7 dias de duracion).

---

## 3. Backtesting con Precios Realistas y Costes de Trading

### NUEVO: Motor de backtesting ahora incluye:
- **Spread por pierna**: Coste del bid/ask spread
- **Slippage**: Impacto de precio al ejecutar
- **Fees de plataforma**: Comisiones de Polymarket
- Uso: `--spread 2 --slippage 0.5 --fee 0`

### 3.1 Comparativa de escenarios de precios (88-112%, 7d, semanal, SL5%)

Los precios originales (0.55/0.65) eran irrealistas. Para un rango 88-112% del spot, los precios reales en Polymarket son mucho mas altos porque la probabilidad de que BTC se mantenga por encima del 88% o por debajo del 112% es alta.

| Escenario | YES_low | YES_high | Coste | Beneficio | ROI | Breakeven WR |
|-----------|---------|----------|-------|-----------|-----|-------------|
| Original (IRREAL) | 0.55 | 0.65 | 0.90 | 1.10 | 122% | 45.0% |
| Agresivo | 0.78 | 0.22 | 1.60 | 0.40 | 25.1% | 79.9% |
| **Realista** | **0.85** | **0.15** | **1.74** | **0.26** | **14.8%** | **87.1%** |
| Conservador | 0.82 | 0.18 | 1.68 | 0.32 | 19.0% | 84.0% |
| Pesimista | 0.88 | 0.12 | 1.80 | 0.20 | 10.9% | 90.2% |

*Todos con spread 2%, slippage 0.5%, sin fees de plataforma*

### 3.2 Resultados por rango y duracion (precios realistas 0.85/0.15, con costes)

| Rango | Dur | WR | PnL | SL | Edge vs BE | Veredicto |
|-------|-----|-----|-----|-----|------------|-----------|
| 88-112% | 3d | **100.0%** | 30.13 | 0 | **+12.9pp** | EXCELENTE |
| 90-110% | 3d | **100.0%** | 30.13 | 0 | **+12.9pp** | EXCELENTE |
| 92-108% | 3d | **100.0%** | 30.13 | 0 | **+12.9pp** | EXCELENTE |
| 88-112% | 5d | **100.0%** | 30.13 | 0 | **+12.9pp** | EXCELENTE |
| 90-110% | 5d | 99.1% | 28.13 | 0 | +12.0pp | MUY BUENO |
| 92-108% | 5d | 96.6% | 22.13 | 0 | +9.5pp | BUENO |
| **88-112%** | **7d** | **99.1%** | **28.13** | **0** | **+12.0pp** | **RECOMENDADO** |
| 90-110% | 7d | 96.6% | 22.13 | 0 | +9.5pp | BUENO |
| 92-108% | 7d | 92.3% | 12.13 | 2 | +5.2pp | MARGINAL |
| 88-112% | 14d | 87.1% | -0.13 | 8 | -0.1pp | BREAKEVEN |
| 90-110% | 14d | 83.6% | -8.13 | 10 | -3.5pp | PIERDE |
| 92-108% | 14d | 76.7% | -24.13 | 17 | -10.4pp | PIERDE |

### 3.3 Resultados sin costes para comparar el impacto

| Rango | Dur | WR (sin costes) | Edge sin costes | Edge con costes | Costes eliminan |
|-------|-----|-----------------|-----------------|-----------------|-----------------|
| 88-112% | 7d | 99.1% | +14.1pp | +12.0pp | 2.1pp |
| 90-110% | 7d | 96.6% | +11.6pp | +9.5pp | 2.1pp |
| 92-108% | 7d | 92.3% | +7.3pp | +5.2pp | 2.1pp |
| 88-112% | 14d | 87.1% | +2.1pp | -0.1pp | 2.2pp → **ELIMINA EL EDGE** |

**Conclusion:** Los costes de trading eliminan ~2pp de edge. Configuraciones con edge <3pp sin costes se vuelven **no rentables** con costes reales.

### 3.4 Sensibilidad a costes de trading (88-112%, 7d, semanal, precios 0.85/0.15)

| Spread | Slippage | Coste efectivo | Profit/trade | ROI | Breakeven WR | Edge |
|--------|----------|---------------|-------------|-----|-------------|------|
| 0% | 0% | 1.7000 | 0.3000 | 17.6% | 85.0% | +14.1pp |
| 1% | 0.5% | 1.7255 | 0.2745 | 15.9% | 86.3% | +12.9pp |
| **2%** | **0.5%** | **1.7425** | **0.2575** | **14.8%** | **87.1%** | **+12.0pp** |
| 3% | 0.5% | 1.7595 | 0.2405 | 13.7% | 88.0% | +11.2pp |
| 4% | 1% | 1.7850 | 0.2150 | 12.0% | 89.2% | +9.9pp |
| 5% | 1% | 1.8020 | 0.1980 | 11.0% | 90.1% | +9.0pp |

**Conclusion:** La estrategia soporta hasta ~5% de spread+slippage combinado y sigue siendo rentable con rango 88-112% y 7d.

### 3.5 Comparativa por nivel de precios (con costes, 7d, semanal)

| Precios | Rango | WR | ROI/trade | Edge | PnL total | Viable? |
|---------|-------|-----|-----------|------|-----------|---------|
| 0.78/0.22 | 88-112% | 99.1% | 25.1% | +19.2pp | 44.92 | SI - mejor caso |
| 0.78/0.22 | 90-110% | 96.6% | 25.1% | +16.6pp | 38.92 | SI |
| 0.78/0.22 | 92-108% | 92.3% | 25.1% | +12.4pp | 28.92 | SI |
| **0.85/0.15** | **88-112%** | **99.1%** | **14.8%** | **+12.0pp** | **28.13** | **SI** |
| 0.85/0.15 | 90-110% | 96.6% | 14.8% | +9.5pp | 22.13 | SI |
| 0.85/0.15 | 92-108% | 92.3% | 14.8% | +5.2pp | 12.13 | MARGINAL |
| 0.82/0.18 | 88-112% | 99.1% | 19.0% | +15.1pp | 35.32 | SI |
| 0.82/0.18 | 92-108% | 92.3% | 19.0% | +8.3pp | 19.32 | SI |
| 0.88/0.12 | 88-112% | 99.1% | 10.9% | +8.9pp | 20.93 | SI - minimo |
| 0.88/0.12 | 92-108% | 92.3% | 10.9% | +2.1pp | 4.93 | APENAS |
| 0.88/0.12 | 88-112% 14d | 87.1% | 10.9% | -3.1pp | -7.26 | NO |

---

## 4. Analisis de Riesgos

### 4.1 Datos historicos sinteticos (LIMITACION CONOCIDA)

Los datos embebidos son interpolaciones lineales con ±2.5% de ruido diario entre anchuras mensuales. Esto **subestima la volatilidad real**:
- No captura crashes repentinos (-15% en un dia)
- Rango diario fijo ~3%, cuando BTC real puede tener 10-20% en dias extremos
- **Impacto estimado:** WR real sera 5-10% menor que el backtested

### 4.2 Tabla de breakeven por escenario

| Nivel de precios | WR necesario (breakeven) | WR backtested (7d, 88-112%) | Margen de seguridad |
|-----------------|--------------------------|----------------------------|---------------------|
| 0.78/0.22 | 79.9% | 99.1% | 19.2pp |
| 0.82/0.18 | 84.0% | 99.1% | 15.1pp |
| **0.85/0.15** | **87.1%** | **99.1%** | **12.0pp** |
| 0.88/0.12 | 90.2% | 99.1% | 8.9pp |

Incluso asumiendo que el WR real cae 10pp (a ~89%), la estrategia sigue siendo rentable con precios hasta 0.85/0.15.

### 4.3 Escenarios de fracaso

| Escenario | Prob. estimada | Perdida por trade | Frecuencia en 2 anos |
|-----------|---------------|-------------------|---------------------|
| BTC crash >12% en 7d | ~1-2% | $1.74 (100%) | 1-2 veces |
| BTC pump >12% en 7d | ~0.5-1% | $1.74 (100%) | 0-1 veces |
| Spread >5% por baja liquidez | Variable | Reduce profit a <$0.20 | Depende del mercado |
| 3 perdidas consecutivas | ~0.001% | $5.22 (3x) | Improbable pero posible |

---

## 5. Ranking de Configuraciones Optimas (CON COSTES REALES)

### Tier 1 - RECOMENDADAS

| # | Config | WR | ROI | Edge | Nota |
|---|--------|-----|-----|------|------|
| 1 | **88-112%, 7d, semanal, SL5%** | 99.1% | 14.8% | +12.0pp | Mejor balance riesgo/reward |
| 2 | **88-112%, 5d, semanal, SL5%** | 100% | 14.8% | +12.9pp | Aun mejor si hay mercados a 5d |
| 3 | **90-110%, 5d, semanal, SL5%** | 99.1% | 14.8% | +12.0pp | Alternativa con rango ligeramente menor |

### Tier 2 - VIABLES

| # | Config | WR | ROI | Edge | Nota |
|---|--------|-----|-----|------|------|
| 4 | 90-110%, 7d, semanal, SL5% | 96.6% | 14.8% | +9.5pp | Buen edge, algo mas de riesgo |
| 5 | 92-108%, 7d, semanal, SL5% | 92.3% | 14.8% | +5.2pp | Edge estrecho pero positivo |

### Tier 3 - EVITAR

| Config | Por que |
|--------|---------|
| Cualquier rango, 14d+ | Edge desaparece con costes reales |
| 92-108%, 7d+ con precios >0.85 | Breakeven demasiado alto |
| 95-105%, cualquier duracion >3d | Rango demasiado estrecho |
| Cualquier config sin dry-run previo | Necesitas validar precios reales primero |

---

## 6. Metricas Avanzadas (Config: 88-112%, 7d, weekly, precios 0.85/0.15, spread 2%, slip 0.5%)

```
Kelly Criterion:
  Full Kelly    : 93.4% del bankroll
  Half Kelly    : 46.7% (recomendado)
  Quarter Kelly : 23.3% (conservador)

Risk-Adjusted Returns:
  Sharpe Ratio  : 9.43
  Sortino Ratio : 10.82
  Max Drawdown  : 1.74 abs (1 trade perdido)
  Profit Factor : 17.14

Monte Carlo (10k simulaciones, 117 trades):
  Median PnL    : 28.13
  5th-95th      : [24.13, 30.13]
  P(profit)     : 100.0%
  Max DD (95th) : 1.94

Expected Value:
  EV/trade      : 0.2404 (13.80%)
  Breakeven WR  : 87.1%
  Actual WR     : 99.1%
  Edge          : +12.0pp
```

---

## 7. Uso del CLI con costes

```bash
# Backtest REALISTA (recomendado)
cargo run -- backtest --offline \
  --low-pct 88 --high-pct 112 \
  --duration-days 7 --interval weekly \
  --yes-price-low 0.85 --yes-price-high 0.15 \
  --stop-loss 5 --take-profit 0 \
  --spread 2 --slippage 0.5

# Backtest OPTIMISTA (precios mas favorables)
cargo run -- backtest --offline \
  --low-pct 88 --high-pct 112 \
  --duration-days 7 --interval weekly \
  --yes-price-low 0.78 --yes-price-high 0.22 \
  --stop-loss 5 \
  --spread 2 --slippage 0.5

# Backtest PESIMISTA (worst case de precios)
cargo run -- backtest --offline \
  --low-pct 88 --high-pct 112 \
  --duration-days 7 --interval weekly \
  --yes-price-low 0.88 --yes-price-high 0.12 \
  --stop-loss 5 \
  --spread 3 --slippage 1
```

---

## 8. Veredicto Final

### La estrategia ES VIABLE para Polymarket, incluso con costes reales.

**Condiciones necesarias:**
1. Usar rango amplio: **88-112%** (no 92-108%)
2. Duracion maxima: **7 dias** (14d+ pierde dinero con costes)
3. Los precios reales YES/NO deben dar un coste por unidad **<$1.80**
4. El spread bid/ask debe ser **<5%** por pierna
5. Empezar SIEMPRE en **dry-run** para calibrar precios reales

**Retorno esperado realista:**
- ~14.8% ROI por trade ganador (con costes)
- ~99% WR en datos historicos (estimar 85-95% en real)
- ~$0.26 de beneficio neto por $1.74 invertido
- Edge de +12pp sobre breakeven (suficiente para absorber un WR real menor)

**El factor clave es la diferencia entre el breakeven WR (87.1%) y el WR esperado real (~89-95%).** Si el WR real cae por debajo del 87.1%, la estrategia pierde dinero. El margen de seguridad de +12pp deberia ser suficiente, pero solo un periodo de dry-run con precios reales puede confirmarlo.

---

*Nuevos flags del CLI: `--spread`, `--slippage`, `--fee` (en porcentaje). El motor de backtesting ahora calcula costes efectivos de entrada incluyendo spread y slippage en ambas piernas, y fees de plataforma en entrada y salida.*
