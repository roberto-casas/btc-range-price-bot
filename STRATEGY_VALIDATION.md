# Validacion de la Estrategia Delta-Neutral BTC Range para Polymarket

**Fecha:** 2026-03-08
**Autor:** Analisis automatizado
**Datos:** 821 velas diarias BTC/USD (Ene 2023 - Abr 2025)

---

## 1. Resumen de la Estrategia

La estrategia explota **dos mercados de prediccion emparejados** en Polymarket para beneficiarse cuando Bitcoin permanece dentro de un rango de precio:

- **Pierna LOW:** COMPRAR YES en "BTC por encima de $X" (donde X esta por debajo del precio actual)
- **Pierna HIGH:** COMPRAR NO en "BTC por encima de $Y" (donde Y esta por encima del precio actual)
- **Condicion de ganancia:** BTC permanece dentro del rango [$X, $Y] al vencimiento

**Ejemplo con BTC a $90,000:**
- LOW: Comprar YES en "BTC > $82,800" (92% del spot) → precio YES = $0.55
- HIGH: Comprar NO en "BTC > $97,200" (108% del spot) → precio NO = $0.35 (1 - 0.65)
- Coste por unidad: $0.55 + $0.35 = $0.90
- Beneficio si en rango: $2.00 - $0.90 = $1.10 (122% ROI)
- Perdida maxima: $0.90 (el coste invertido)

---

## 2. Mercados Disponibles en Polymarket

### Tipos de mercados Bitcoin activos:

| Tipo | Timeframe | Estructura | Relevancia para la estrategia |
|------|-----------|------------|-------------------------------|
| **"Bitcoin above __ on [fecha]?"** | Diario | YES/NO por umbral | **IDEAL** - Es exactamente lo que usa el bot |
| **"Bitcoin price on [fecha]?"** | Diario | Multiples rangos | Alternativa, pero estructura diferente |
| **"What price will Bitcoin hit in [mes]?"** | Mensual | 20-30 outcomes | No compatible directamente |
| **"Bitcoin Up or Down - 5 Minutes"** | 5 min | Binario up/down | Demasiado rapido, no aplica |
| **"Bitcoin above __ this week?"** | Semanal | YES/NO por umbral | **IDEAL** - Mejor liquidez |

### Timeframes disponibles en Polymarket Crypto:
- **5 Minutos** - Demasiado rapido, sin edge real
- **15 Minutos / 1 Hora / 4 Horas** - Ultra-corto plazo
- **Diario** (7 mercados activos) - **Bueno para la estrategia**
- **Semanal** (63 mercados activos) - **OPTIMO para la estrategia**
- **Mensual** (22 mercados activos) - Riesgo elevado, peor win rate
- **Anual** (21 mercados) - No compatible

### Conclusion sobre timeframes:
El bot genera slugs tipo `bitcoin-above-on-{month}-{day}` que matchean perfectamente con los mercados diarios "Bitcoin above __ on [fecha]?". Los mercados semanales (63 activos) ofrecen la **mejor liquidez** y son el timeframe ideal.

---

## 3. Resultados del Backtesting Exhaustivo

### 3.1 Sin Stop-Loss ni Take-Profit (hold to expiry)

| Rango | Duracion | Trades | Win Rate | PnL Total | Edge vs Breakeven |
|-------|----------|--------|----------|-----------|-------------------|
| 88-112% | 1d | 820 | **100.0%** | 902.0 | +55.0pp |
| 90-110% | 1d | 820 | **100.0%** | 902.0 | +55.0pp |
| 92-108% | 1d | 820 | **100.0%** | 902.0 | +55.0pp |
| 95-105% | 1d | 820 | **100.0%** | 902.0 | +55.0pp |
| 88-112% | 3d | 818 | **100.0%** | 899.8 | +55.0pp |
| 90-110% | 3d | 818 | **100.0%** | 899.8 | +55.0pp |
| 92-108% | 3d | 818 | **100.0%** | 899.8 | +55.0pp |
| 95-105% | 3d | 818 | 98.0% | 867.8 | +53.0pp |
| 88-112% | 5d | 816 | **100.0%** | 897.6 | +55.0pp |
| 90-110% | 5d | 816 | 99.4% | 887.6 | +54.4pp |
| 92-108% | 5d | 816 | 97.4% | 855.6 | +52.4pp |
| 95-105% | 5d | 816 | 90.8% | 747.6 | +45.8pp |
| 88-112% | 7d | 814 | **99.9%** | 893.4 | +54.9pp |
| 90-110% | 7d | 814 | 96.9% | 845.4 | +51.9pp |
| 92-108% | 7d | 814 | 93.0% | 781.4 | +48.0pp |
| 95-105% | 7d | 814 | 81.4% | 593.4 | +36.4pp |
| 88-112% | 14d | 807 | 88.4% | 699.7 | +43.4pp |
| 90-110% | 14d | 807 | 83.5% | 621.7 | +38.5pp |
| 92-108% | 14d | 807 | 78.3% | 537.7 | +33.3pp |
| 95-105% | 14d | 807 | 62.0% | 273.7 | +17.0pp |
| 88-112% | 30d | 791 | 71.8% | 424.1 | +26.8pp |
| 90-110% | 30d | 791 | 64.3% | 306.1 | +19.3pp |
| 92-108% | 30d | 791 | 52.7% | 122.1 | +7.7pp |
| **95-105%** | **30d** | **791** | **33.2%** | **-185.9** | **-11.8pp** |

### 3.2 Con SL5% y TP80% (entrada semanal)

| Rango | Duracion | Trades | Win Rate | PnL | Stop-Loss | Take-Profit | Edge |
|-------|----------|--------|----------|-----|-----------|-------------|------|
| 90-110% | 1d | 117 | **100.0%** | 102.96 | 0 | 117 | +55.0pp |
| 92-108% | 1d | 117 | **100.0%** | 102.96 | 0 | 117 | +55.0pp |
| 88-112% | 1d | 117 | **100.0%** | 102.96 | 0 | 117 | +55.0pp |
| 90-110% | 3d | 117 | **100.0%** | 102.96 | 0 | 117 | +55.0pp |
| 92-108% | 3d | 117 | **100.0%** | 102.96 | 0 | 117 | +55.0pp |
| 88-112% | 3d | 117 | **100.0%** | 102.96 | 0 | 117 | +55.0pp |
| 90-110% | 7d | 117 | **100.0%** | 102.96 | 0 | 117 | +55.0pp |
| 92-108% | 7d | 117 | 98.3% | 99.40 | 1 | 115 | +53.3pp |
| 88-112% | 7d | 117 | **100.0%** | 102.96 | 0 | 117 | +55.0pp |
| 90-110% | 14d | 116 | 96.6% | 94.96 | 3 | 112 | +51.6pp |
| 92-108% | 14d | 116 | 93.1% | 87.84 | 8 | 108 | +48.1pp |
| 88-112% | 14d | 116 | 98.3% | 98.52 | 2 | 114 | +53.3pp |

---

## 4. Analisis Critico de la Estrategia

### 4.1 Fortalezas

1. **Win rate extremadamente alto** en duraciones cortas (1-7 dias): >96% con rangos ±8-12%
2. **Edge consistente sobre breakeven**: La tasa de breakeven es 45%, y la estrategia mantiene >93% WR en la mayoria de configuraciones
3. **Risk/Reward favorable**: Coste $0.90 por ganancia de $1.10 = ratio 1.22:1
4. **Delta-neutral**: No depende de la direccion del precio, solo de que se mantenga en rango
5. **Kelly criterion alto**: Full Kelly >96% indica un edge masivo

### 4.2 Debilidades y Riesgos CRITICOS

#### Problema 1: Datos historicos sinteticos
Los datos embebidos son **interpolaciones lineales con ruido pseudo-aleatorio (±2.5%)** entre puntos de anclaje mensuales. Esto **subestima dramaticamente la volatilidad real de BTC**:
- El ruido diario del 2.5% es artificialmente bajo
- No captura crashes repentinos (ej: -15% en un dia como Luna/FTX)
- No captura wicks extremos intradiarios
- Los datos OHLC generados tienen un rango diario fijo del 3%, cuando BTC real puede tener rangos del 10-20% en dias volatiles

**Impacto:** Los win rates del 98-100% estan **inflados** por datos demasiado suaves. Con datos reales de mercado, especialmente en periodos volatiles, el win rate seria significativamente menor.

#### Problema 2: Precios YES/NO asumidos
El backtesting usa precios fijos (YES_LOW=0.55, YES_HIGH=0.65), pero en Polymarket real:
- Los precios varian segun la distancia al umbral y el tiempo hasta el vencimiento
- Mercados con thresholds cerca del spot tienen precios ~0.50 (poca ganancia)
- Mercados lejanos al spot (>10%) pueden tener precios <0.10 o >0.90
- El spread bid/ask puede ser significativo (1-5%)
- La liquidez varia enormemente entre mercados

#### Problema 3: Costes de ejecucion ignorados
- No se considera el spread bid/ask
- No se consideran las fees de Polymarket
- No se considera el slippage en ejecucion
- En mercados poco liquidos, mover el precio es inevitable

#### Problema 4: Riesgo de correlacion temporal
- BTC tiende a tener periodos de baja volatilidad seguidos de explosiones
- La estrategia funciona perfecto en lateralizacion pero falla en breakouts
- Eventos macro (halving, regulacion, black swans) pueden invalidar historicos

### 4.3 Escenarios de Fracaso

| Escenario | Probabilidad | Impacto |
|-----------|-------------|---------|
| BTC crash >8% en 7 dias | ~5-10% por trade | Perdida total (-$0.90/unidad) |
| BTC pump >8% en 7 dias | ~3-5% por trade | Perdida total (-$0.90/unidad) |
| Flash crash intradiario | ~2% por trade | SL activado incorrectamente |
| Baja liquidez en mercado | Frecuente | Precios peores, menor profit |
| Cambio en estructura de mercados Polymarket | Posible | Bot no encuentra pares |

---

## 5. Ranking de Configuraciones Optimas

### Tier 1 - MEJOR RELACION RIESGO/BENEFICIO (Recomendadas)

| # | Config | Por que |
|---|--------|---------|
| 1 | **88-112%, 7d, semanal, SL5%, TP80%** | WR 100%, rango amplio absorbe volatilidad, 117 trades en 2 anos |
| 2 | **90-110%, 7d, semanal, SL5%, TP80%** | WR 100%, buen balance rango/beneficio |
| 3 | **90-110%, 5d, diario, sin TP** | WR 99.4%, duracion corta reduce exposicion |

### Tier 2 - BUENAS OPCIONES

| # | Config | Por que |
|---|--------|---------|
| 4 | 92-108%, 7d, semanal, SL5%, TP80% | WR 98.3%, solo 1 SL, configuracion actual del bot |
| 5 | 88-112%, 14d, semanal, SL5%, TP80% | WR 98.3%, buen para mercados semanales |
| 6 | 90-110%, 3d, diario | WR 100%, ideal si hay mercados con vencimiento a 3 dias |

### Tier 3 - EVITAR

| # | Config | Por que |
|---|--------|---------|
| - | 95-105%, >5d | Rango demasiado estrecho, WR cae por debajo del 90% |
| - | Cualquier rango, 30d | WR cae drasticamente, BTC se mueve demasiado en 1 mes |
| - | 92-108%, 14d+ | WR <93%, el edge se estrecha peligrosamente |

---

## 6. Timeframes Optimos en Polymarket

Basado en los mercados disponibles actualmente:

### Mercados "Bitcoin above __ on [fecha]?" (DIARIOS)
- **Disponibilidad:** 7 mercados activos diarios
- **Duracion real:** 1-7 dias desde entrada hasta vencimiento
- **Mejor config:** Rango 88-112%, entrar el lunes con vencimiento el viernes/sabado
- **Ventaja:** Maxima granularidad, el bot ya genera los slugs correctos
- **Desventaja:** Liquidez variable, posiblemente spread alto

### Mercados semanales (63 activos)
- **Disponibilidad:** La mayor cantidad de mercados
- **Duracion real:** 5-7 dias
- **Mejor config:** Rango 88-112% o 90-110%, SL5%, TP80%
- **Ventaja:** Mayor liquidez por ser el formato mas popular
- **Esto es lo que el bot deberia priorizar**

### Mercados mensuales (22 activos)
- **Disponibilidad:** Menor cantidad
- **Duracion real:** 14-30 dias
- **Mejor config:** Solo con rango 88-112% (WR ~88%)
- **Desventaja:** Win rate cae significativamente, no recomendado

---

## 7. Recomendaciones Finales

### Es viable la estrategia? SI, pero con matices importantes.

**Argumentos a favor:**
1. El edge matematico es real: breakeven en 45% WR, la estrategia historicamente supera 93%+
2. Los mercados semanales de Polymarket ofrecen exactamente la estructura necesaria
3. Con ±10-12% de rango y 7 dias, BTC raramente se sale del rango
4. El bot esta bien construido: tiene SL, TP, dry-run, analytics avanzadas

**Argumentos en contra / advertencias:**
1. **Los backtests sobreestiman el WR** por usar datos sinteticos demasiado suaves
2. **En la realidad, espera un WR de 80-90%**, no 98-100%
3. **Los precios YES/NO reales variaran** significativamente de los asumidos (0.55/0.65)
4. **El ROI real por trade sera menor** despues de spreads, fees y slippage
5. **Black swans pueden causar multiples perdidas consecutivas** (ej: si BTC cae 20% en una semana)

### Plan de accion sugerido:

1. **Empezar en modo dry-run** durante 2-4 semanas para calibrar precios reales
2. **Usar rango 88-112% (no 92-108%)** para mayor margen de seguridad
3. **Preferir mercados semanales** (7 dias) sobre diarios o mensuales
4. **Limitar capital por trade** al 5-10% del bankroll (Half Kelly sugiere ~48%)
5. **Monitorizar volatilidad** (ATR>5% = precaucion, ATR>7% = no entrar)
6. **Obtener datos de precios reales de CLOB** antes de cada entrada
7. **Validar con datos historicos reales de CoinGecko** (no los embebidos) usando `--history-days 365`

---

## 8. Metricas Avanzadas (Config Optima: 92-108%, 7d, weekly)

```
Kelly Criterion:
  Full Kelly    : 96.5% del bankroll
  Half Kelly    : 48.3% (recomendado)
  Quarter Kelly : 24.1% (conservador)

Risk-Adjusted Returns:
  Sharpe Ratio  : 26.59
  Sortino Ratio : 52.36
  Max Drawdown  : 1.8% (0.90 abs)
  Calmar Ratio  : 49.65
  Profit Factor : 56.22

Monte Carlo (10k simulaciones):
  Median PnL    : 99.40
  5th-95th      : [95.84, 102.96]
  P(profit)     : 100.0%
  Max DD (95th) : 0.92

Expected Value:
  EV/trade      : 1.0658 (118.42%)
  Breakeven WR  : 45.0%
  Actual WR     : 98.3%
  Edge          : +53.3pp
```

---

*Nota: Este analisis utiliza datos historicos sinteticos. Se recomienda fuertemente repetir el backtesting con datos reales de CoinGecko (`cargo run -- backtest --history-days 365`) antes de operar con capital real.*
