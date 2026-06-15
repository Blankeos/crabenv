FROM python:3.12-slim

WORKDIR /app
COPY pyproject.toml ./
COPY src ./src

ENV PYTHON_LOG_LEVEL=debug

CMD ["python", "-m", "python_service.main"]

