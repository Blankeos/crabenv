FROM node:22-slim

WORKDIR /app
COPY package.json ./
COPY src ./src

ENV HONO_PORT=8890
EXPOSE 8890

CMD ["node", "src/index.ts"]

