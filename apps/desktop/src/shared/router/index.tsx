import React, { createContext, useContext, useEffect, useState, useMemo } from "react";

interface LocationContextType {
  pathname: string;
  search: string;
  hash: string;
}

const LocationContext = createContext<LocationContextType>({
  pathname: "/",
  search: "",
  hash: "",
});

const ParamsContext = createContext<Record<string, string>>({});

function parseHash(hash: string): { pathname: string; search: string } {
  const clean = hash.replace(/^#/, "") || "/";
  const qIndex = clean.indexOf("?");
  if (qIndex === -1) {
    return { pathname: clean.startsWith("/") ? clean : `/${clean}`, search: "" };
  }
  const pathname = clean.slice(0, qIndex);
  return {
    pathname: pathname.startsWith("/") ? pathname : `/${pathname}`,
    search: clean.slice(qIndex),
  };
}

export const HashRouter: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [loc, setLoc] = useState(() => {
    if (typeof window === "undefined") {
      return { pathname: "/", search: "", hash: "" };
    }
    const parsed = parseHash(window.location.hash);
    return { ...parsed, hash: window.location.hash };
  });

  useEffect(() => {
    const onHashChange = () => {
      const parsed = parseHash(window.location.hash);
      setLoc({ ...parsed, hash: window.location.hash });
    };

    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  return (
    <LocationContext.Provider value={loc}>
      {children}
    </LocationContext.Provider>
  );
};

export function useLocation(): LocationContextType {
  return useContext(LocationContext);
}

export function useNavigate() {
  return (to: string | number) => {
    if (typeof to === "number") {
      window.history.go(to);
    } else {
      const targetHash = to.startsWith("/") ? `#${to}` : `#/${to}`;
      window.location.hash = targetHash;
    }
  };
}

export function useParams<T extends Record<string, string>>(): T {
  return useContext(ParamsContext) as T;
}

export function matchPath(
  pattern: string,
  pathname: string
): { matches: boolean; params: Record<string, string> } {
  // Normalize
  const pSegs = pattern.split("/").filter(Boolean);
  const uSegs = pathname.split("/").filter(Boolean);

  if (pSegs.length !== uSegs.length) {
    // Check wildcard
    if (pattern.endsWith("/*")) {
      const baseSegs = pattern.slice(0, -2).split("/").filter(Boolean);
      if (uSegs.length >= baseSegs.length) {
        let match = true;
        const params: Record<string, string> = {};
        for (let i = 0; i < baseSegs.length; i++) {
          if (baseSegs[i].startsWith(":")) {
            params[baseSegs[i].slice(1)] = uSegs[i];
          } else if (baseSegs[i] !== uSegs[i]) {
            match = false;
            break;
          }
        }
        if (match) {
          return { matches: true, params };
        }
      }
    }
    return { matches: false, params: {} };
  }

  const params: Record<string, string> = {};
  for (let i = 0; i < pSegs.length; i++) {
    if (pSegs[i].startsWith(":")) {
      params[pSegs[i].slice(1)] = decodeURIComponent(uSegs[i]);
    } else if (pSegs[i] !== uSegs[i]) {
      return { matches: false, params: {} };
    }
  }

  return { matches: true, params };
}

export interface RouteProps {
  path: string;
  element: React.ReactNode;
}

export const Route: React.FC<RouteProps> = () => {
  return null;
};

export const Routes: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const { pathname } = useLocation();

  const childArray = React.Children.toArray(children) as React.ReactElement<RouteProps>[];

  for (const child of childArray) {
    if (!React.isValidElement(child)) continue;
    const { path, element } = child.props;
    const { matches, params } = matchPath(path, pathname);

    if (matches) {
      return (
        <ParamsContext.Provider value={params}>
          {element}
        </ParamsContext.Provider>
      );
    }
  }

  return null;
};

export const Navigate: React.FC<{ to: string; replace?: boolean }> = ({ to, replace }) => {
  useEffect(() => {
    const targetHash = to.startsWith("/") ? `#${to}` : `#/${to}`;
    if (replace) {
      window.location.replace(targetHash);
    } else {
      window.location.hash = targetHash;
    }
  }, [to, replace]);

  return null;
};

export interface LinkProps extends React.AnchorHTMLAttributes<HTMLAnchorElement> {
  to: string;
  className?: string;
  activeClassName?: string;
}

export const Link: React.FC<LinkProps> = ({
  to,
  className = "",
  activeClassName = "",
  children,
  ...props
}) => {
  const { pathname } = useLocation();
  const targetPath = to.startsWith("/") ? to : `/${to}`;
  const isActive = pathname === targetPath || (targetPath !== "/" && pathname.startsWith(targetPath));

  const combinedClass = useMemo(() => {
    return `${className} ${isActive ? activeClassName : ""}`.trim();
  }, [className, activeClassName, isActive]);

  return (
    <a href={`#${targetPath}`} className={combinedClass} {...props}>
      {children}
    </a>
  );
};
