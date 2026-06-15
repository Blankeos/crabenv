from python_service.env import settings


def main() -> None:
    print(
        {
            "database": settings.database_url,
            "origin": settings.allowed_origin,
            "log_level": settings.log_level,
        }
    )


if __name__ == "__main__":
    main()

