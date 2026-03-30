FROM node:20-slim AS builder

WORKDIR /app

COPY package.json ./
RUN npm install

COPY . .
RUN npm run build

FROM node:20-slim

RUN groupadd -g 1000 appuser && useradd -u 1000 -g appuser -s /bin/sh appuser

WORKDIR /app

COPY --from=builder /app/dist ./dist
COPY --from=builder /app/node_modules ./node_modules

RUN chown -R appuser:appuser /app

USER appuser

EXPOSE 3100

CMD ["node", "./dist/server/entry.mjs"]
