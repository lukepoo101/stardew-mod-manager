"""Native packaged-application smoke tests.

The package is platform-parameterised: :mod:`common` owns the WebDriver client and
the user-journey assertions, and each platform module owns the driver process,
the application launch capabilities and the fixture layout.
"""
