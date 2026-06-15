FROM node:22-slim

WORKDIR /app
COPY package.json ./
COPY src ./src

ENV HONO_API_PORT=8787
EXPOSE 8787

CMD ["node", "src/index.ts"]

