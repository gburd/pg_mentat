from setuptools import setup, find_packages

setup(
    name="pg-mentat",
    version="0.1.0",
    description="Idiomatic Python client for pg_mentat -- Direct PostgreSQL access",
    long_description=open("README.md").read() if __import__("os").path.exists("README.md") else "",
    long_description_content_type="text/markdown",
    author="pg_mentat contributors",
    url="https://codeberg.org/gregburd/pg_mentat",
    license="Apache-2.0",
    packages=find_packages(exclude=["tests", "tests.*"]),
    python_requires=">=3.7",
    install_requires=[
        "psycopg2-binary>=2.9.0",
    ],
    extras_require={
        "dev": ["pytest>=7.0", "pytest-cov"],
    },
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: Apache Software License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.7",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
        "Topic :: Database",
    ],
)
