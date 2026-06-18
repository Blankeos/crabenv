from basic_python.env import settings


def main() -> None:
    print(f"basic-python running against {settings.database_url}")
