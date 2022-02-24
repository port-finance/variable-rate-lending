port-lending-cli \
--program pdQ2rQQU5zH2rDgZ7xH2azMBJegUzUyunJ5Jd637hC4 \
--fee-payer    ~/.config/solana/id.json \
add-reserve \
--market-owner usb://ledger \
--source-owner ~/.config/solana/id.json \
--market       5MKr6PVttXFXcVvxbPK5puhsYKyQvSKDBmJRkqGxuJb4 \
--source       5dMTXsP8q7oCVCYHj9owG3woyeGDhqe7jP3wkHYcEV8U \
--amount       1.0  \
--loan-to-value-ratio 70 \
--liquidation-threshold 80 \
--liquidation-bonus 10 \
--optimal-utilization-rate 80 \
--min-borrow-rate 0 \
--optimal-borrow-rate 4 \
--max-borrow-rate 60 \
--pyth-price 7teETxN9Y8VK6uJxsctHEwST75mKLLwPH1jaFdvTQCpD
