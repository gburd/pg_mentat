from setuptools import setup, find_packages

setup(
    name="pg-mentat-client",
    version="0.1.0",
    description="Datomic-compatible Python client for pg_mentat",
    long_description=open("README.md").read() if __import__("os").path.exists("README.md") else "",
    long_description_content_type="text/markdown",
    author="pg_mentat contributors",
    url="https://codeberg.org/gregburd/pg_mentat",
    license="Apache-2.0",
    packages=find_packages(),
    python_requires=">=3.10",
    install_requires=[
        "websocket-client>=1.0",
    ],
    extras_require={
        "async": ["websockets>=11.0"],
        "dev": ["pytest", "pytest-asyncio"],
    },
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: Apache Software License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
        "Topic :: Database",
    ],
)
